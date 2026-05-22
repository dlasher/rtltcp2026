use std::io::prelude::*;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::result::Result as StdResult;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;
#[cfg(feature = "systemd")]
use listenfd::ListenFd;
use tracing::{debug, info, warn};
use tokio::sync::broadcast;

mod error;
mod control;
pub mod stream;
use crate::control::*;
use crate::error::RtlTcpError;

#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about = "an I/Q spectrum server for RTL2832 based DVB-T receivers",
    long_about = None
)]
struct Args {
    /// operating mode: "serve" (default) or "proxy"
    #[clap(long, default_value = "serve",
           value_parser = clap::builder::PossibleValuesParser::new(&["serve", "proxy"]))]
    mode: String,

    /// listen address
    #[clap(short, long, default_value = "127.0.0.1")]
    address: String,

    /// master port — accepts the driver connection (alias for --port)
    #[clap(short = 'p', long = "master-port", alias = "port", default_value_t = 1234)]
    master_port: u16,

    /// slave port — accepts read-only consumer connections
    #[clap(long)]
    slave_port: Option<u16>,

    /// maximum number of connected slaves
    #[clap(long, default_value_t = 10)]
    max_slaves: usize,

    /// upstream rtltcp server (host:port) for proxy mode
    #[clap(long)]
    upstream: Option<String>,

    /// hex-encoded 32-byte encryption key
    #[clap(long, conflicts_with = "key_file")]
    key: Option<String>,

    /// path to 32-byte raw encryption key file
    #[clap(long, conflicts_with = "key")]
    key_file: Option<String>,

    /// device index
    #[clap(short, long, default_value_t = 0)]
    device_index: u32,

    /// number of decoding buffers
    #[clap(short, long, default_value_t = 15)]
    buffers: u32,

    /// tcp sending buffer size (bytes) [default: 512000, range: 1-10485760]
    #[clap(short = 's', long, default_value_t = 512000)]
    tcp_buffers: usize,

    /// socket read timeout in seconds
    #[clap(long, default_value_t = 30)]
    read_timeout: u64,

    /// socket write timeout in seconds
    #[clap(long, default_value_t = 30)]
    write_timeout: u64,

    /// IP whitelist (CIDR notation)
    #[clap(long)]
    whitelist: Vec<String>,
}

fn main() -> StdResult<(), RtlTcpError> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    validate_args(&args)?;

    match args.mode.as_str() {
        "proxy" => {
            Err(RtlTcpError::Config("proxy mode not yet implemented".to_string()))
        }
        "serve" => {
            if args.slave_port.is_some() {
                run_serve_multi(args)
            } else {
                run_serve_single(args)
            }
        }
        _ => Err(RtlTcpError::Config(format!("unknown mode: {}", args.mode))),
    }
}

/// Validate CLI argument ranges
fn validate_args(args: &Args) -> StdResult<(), RtlTcpError> {
    if args.buffers == 0 || args.buffers > 32 {
        return Err(RtlTcpError::Config(
            "buffers must be between 1 and 32".to_string(),
        ));
    }
    if args.tcp_buffers == 0 || args.tcp_buffers > 10_485_760 {
        return Err(RtlTcpError::Config(
            "tcp_buffers must be between 1 and 10485760 (10MB)".to_string(),
        ));
    }
    if args.read_timeout == 0 {
        return Err(RtlTcpError::Config(
            "read_timeout must be greater than 0".to_string(),
        ));
    }
    if args.write_timeout == 0 {
        return Err(RtlTcpError::Config(
            "write_timeout must be greater than 0".to_string(),
        ));
    }
    Ok(())
}

/// Bind master port with systemd socket activation support
fn bind_master_port(args: &Args) -> StdResult<TcpListener, RtlTcpError> {
    let addr = format!("{}:{}", args.address, args.master_port);
    #[cfg(feature = "systemd")]
    {
        let mut listenfd = ListenFd::from_env();
        if let Some(listener) = listenfd.take_tcp_listener(0).map_err(|e| {
            RtlTcpError::Config(format!("could not get fd from env: {e}"))
        })? {
            systemd::daemon::notify(false, [(systemd::daemon::STATE_READY, "1")].iter())?;
            return Ok(listener);
        }
    }
    TcpListener::bind(&addr).map_err(Into::into)
}

/// Single-client serve mode (original behavior)
fn run_serve_single(args: Args) -> StdResult<(), RtlTcpError> {
    let read_timeout = Duration::from_secs(args.read_timeout);
    let write_timeout = Duration::from_secs(args.write_timeout);

    // Warn when binding to all interfaces
    let is_all_interfaces = args.address == "0.0.0.0"
        || args.address == "::"
        || args.address == "[::]"
        || args.address.is_empty();
    if is_all_interfaces {
        warn!(
            "binding to all interfaces ({}) — this exposes the server to all network interfaces",
            args.address
        );
    }

    let listener = bind_master_port(&args)?;
    info!("waiting for connection…");
    let (stream, addr) = listener.accept()?;

    let client_ip = addr.ip().to_canonical().to_string();
    if let Err(e) = check_whitelist(&client_ip, &args.whitelist) {
        warn!(target: "rtltcp", "Connection from {client_ip} refused — not in whitelist");
        return Err(e);
    }

    info!("connection from {addr}");
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(write_timeout))?;

    // Task 2.2: Clone the stream for signal-based shutdown.
    // This allows the signal handler to interrupt blocking reads immediately,
    // rather than waiting for the read timeout to expire.
    let stream_for_shutdown = Arc::new(Mutex::new(Some(stream.try_clone()?)));
    let stream_for_shutdown_ctrlc = stream_for_shutdown.clone();

    let (ctl, mut reader) = rtlsdr_mt::open(args.device_index)
        .map_err(|e| RtlTcpError::Device(format!("could not open RTL-SDR device: {e:?}")))?;
    let ctl = Arc::new(Mutex::new(ctl));

    let (sender, receiver) = sync_channel(1);
    let sender_ctrlc = sender.clone();
    let should_exit = Arc::new(AtomicBool::new(false));
    let should_exit_ctrlc = should_exit.clone();
    let agc_state = Arc::new(AgcState::new());

    ctrlc::set_handler(move || {
        info!("received signal, shutting down");
        match sender_ctrlc.try_send(()) {
            Ok(_) => {}
            Err(_) => {
                warn!("could not send exit signal, exiting immediately");
                should_exit_ctrlc.store(true, Ordering::SeqCst);
            }
        }
        // Task 2.2: Shutdown the TCP stream to interrupt any blocking reads.
        // This causes read_exact to return immediately with a connection error,
        // allowing the control thread to check should_exit and break cleanly.
        if let Ok(stream_opt) = stream_for_shutdown_ctrlc.lock() {
            if let Some(ref stream) = *stream_opt {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    })
    .map_err(|e| RtlTcpError::Config(format!("could not set signal handler: {e}")))?;

    // Task 2.3: Track unknown commands for better visibility
    let unknown_command_count = Arc::new(Mutex::new(0u64));

    let thread_ctl = std::thread::spawn({
        let ctl = ctl.clone();
        let should_exit = should_exit.clone();
        let unknown_command_count = unknown_command_count.clone();
        let agc_state = agc_state.clone();
        let mut stream = stream.try_clone()?;
        move || {
            let mut buf = [0u8; COMMAND_HEADER_SIZE];
            let mut rate_limiter = RateLimiter::new(COMMAND_RATE_LIMIT_INTERVAL);
            loop {
                match stream.read_exact(&mut buf) {
                    Ok(()) => {}
                    Err(e)
                        if e.kind() == ErrorKind::UnexpectedEof
                            || e.kind() == ErrorKind::ConnectionReset
                            || e.kind() == ErrorKind::BrokenPipe
                            || e.kind() == ErrorKind::TimedOut
                            || e.kind() == ErrorKind::ConnectionAborted
                            || e.kind() == ErrorKind::NotConnected =>
                    {
                        info!("client disconnected: {e}");
                        break;
                    }
                    Err(e) => {
                        warn!("read error from client: {e}");
                        break;
                    }
                }

                if should_exit.load(Ordering::SeqCst) {
                    info!("exit flag set, stopping control thread");
                    break;
                }

                // Rate limiting check
                if !rate_limiter.check() {
                    debug!("command rate limited, skipping");
                    continue;
                }

                let cmd = buf[0];
                let payload: [u8; 4] = [buf[1], buf[2], buf[3], buf[4]];

                match cmd {
                    CMD_SET_FREQUENCY => {
                        let freq = u32::from_be_bytes(payload);
                        if let Err(e) = validate_frequency(freq) {
                            warn!("invalid frequency: {e}");
                            continue;
                        }
                        info!("setting center freq to {freq}");
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.set_center_freq(freq) {
                                warn!("failed to set center freq: {e:?}");
                            }
                        });
                    }
                    CMD_SET_SAMPLE_RATE => {
                        let sample_rate = u32::from_be_bytes(payload);
                        if let Err(e) = validate_sample_rate(sample_rate) {
                            warn!("invalid sample rate: {e}");
                            continue;
                        }
                        info!("setting sample rate to {sample_rate}");
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.set_sample_rate(sample_rate) {
                                warn!("failed to set sample rate: {e:?}");
                            }
                        });
                    }
                    CMD_SET_GAIN_MODE => {
                        let gain_mode = i32::from_be_bytes(payload);
                        if gain_mode > 0 {
                            if agc_state.disable() {
                                info!("gain mode set to manual (AGC off)");
                            }
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.disable_agc() {
                                    warn!("failed to disable AGC: {e:?}");
                                }
                            });
                        } else {
                            if agc_state.enable() {
                                info!("gain mode set to automatic (AGC on)");
                            }
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.enable_agc() {
                                    warn!("failed to enable AGC: {e:?}");
                                }
                            });
                        }
                    }
                    CMD_SET_TUNER_GAIN => {
                        let gain = i32::from_be_bytes(payload);
                        if let Err(e) = validate_tuner_gain(gain) {
                            warn!("invalid tuner gain: {e}");
                            continue;
                        }
                        info!("setting manual gain to {gain}");
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.set_tuner_gain(gain) {
                                warn!("failed to set tuner gain: {e:?}");
                            }
                        });
                    }
                    CMD_SET_PPM => {
                        let ppm = i32::from_be_bytes(payload);
                        if let Err(e) = validate_ppm(ppm) {
                            warn!("invalid ppm: {e}");
                            continue;
                        }
                        info!("setting ppm to {ppm}");
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.set_ppm(ppm) {
                                warn!("failed to set ppm: {e:?}");
                            }
                        });
                    }
                    CMD_SET_AGC => {
                        let agc = u32::from_be_bytes(payload) == 1u32;
                        if agc {
                            if agc_state.enable() {
                                info!("setting automatic gain control to on");
                            }
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.enable_agc() {
                                    warn!("failed to enable AGC: {e:?}");
                                }
                            });
                        } else {
                            if agc_state.disable() {
                                info!("setting automatic gain control to off");
                            }
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.disable_agc() {
                                    warn!("failed to disable AGC: {e:?}");
                                }
                            });
                        }
                     }
                     CMD_CHAIN_DETECT => {
                         info!("chain detection probe from downstream proxy");
                         if let Err(e) = stream.write_all(&[CMD_CHAIN_DETECT, 0x00, 0x00, 0x00, 0x00]) {
                             warn!("failed to send chain detect ack: {e}");
                         }
                     }
                     _ => {
                        // Task 2.3: Changed from debug! to warn! and added counter
                        let mut count = unknown_command_count.lock().unwrap();
                        *count += 1;
                        warn!("recv unsupported command {buf:?} (total unknown commands: {count})");
                     }
                }
            }
            info!("control thread exiting");
        }
    });

    let thread_cancel = std::thread::spawn({
        let ctl = ctl.clone();
        let should_exit = should_exit.clone();
        move || {
            let _ = receiver.recv();
            info!("stopping read from device");
            if let Ok(mut guard) = ctl.lock() {
                guard.cancel_async_read();
            }
            should_exit.store(true, Ordering::SeqCst);
        }
    });

    let mut buf_write_stream = BufWriter::with_capacity(args.tcp_buffers, stream);
    buf_write_stream.write_all(MAGIC_PACKET)?;
    buf_write_stream.flush()?;

    let total_bytes_sent = Arc::new(AtomicU64::new(0));
    let read_result = reader.read_async(args.buffers, 0, |bytes| {
        let sent =
            total_bytes_sent.fetch_add(bytes.len() as u64, Ordering::Relaxed) + bytes.len() as u64;
        if let Err(e) = buf_write_stream.write_all(bytes) {
            warn!("stream write failed after {sent} bytes, triggering shutdown: {e}",);
            let _ = sender.try_send(());
            return;
        }
        if let Err(e) = buf_write_stream.flush() {
            warn!("flush failed after {sent} bytes, triggering shutdown: {e}",);
            let _ = sender.try_send(());
        }
    });

    // Signal cancel thread so it doesn't hang on recv() if read_async completes normally
    let _ = sender.try_send(());

    let final_bytes = total_bytes_sent.load(Ordering::Relaxed);
    info!("read_async completed after sending {final_bytes} bytes");

    if let Err(e) = read_result {
        warn!("read_async error after {final_bytes} bytes: {e:?}");
    }

    if let Err(e) = thread_cancel.join() {
        warn!("cancel thread panicked: {e:?}");
    }
    if let Err(e) = thread_ctl.join() {
        warn!("control thread panicked: {e:?}");
    }

    info!("rtltcp shut down successfully");
    Ok(())
}

/// Multi-client serve mode with master+slave ports and broadcast fan-out to slaves
fn run_serve_multi(args: Args) -> StdResult<(), RtlTcpError> {
    let read_timeout = Duration::from_secs(args.read_timeout);

    // Warn when binding to all interfaces
    let is_all = args.address == "0.0.0.0" || args.address == "::" || args.address == "[::]" || args.address.is_empty();
    if is_all { warn!("binding to all interfaces ({}) — exposes server to all networks", args.address); }

    let (ctl, mut reader) = rtlsdr_mt::open(args.device_index)
        .map_err(|e| RtlTcpError::Device(format!("could not open RTL-SDR device: {e:?}")))?;
    let ctl = Arc::new(Mutex::new(ctl));
    let agc_state = Arc::new(AgcState::new());
    let magic_packet = MAGIC_PACKET;

    let (tx, _rx) = stream::new_broadcast(stream::DEFAULT_BROADCAST_CAPACITY);
    let should_exit = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = sync_channel(1);
    let all_streams: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));

    // Signal handler: sets exit flag
    ctrlc::set_handler({
        let s = sender.clone();
        let exit = should_exit.clone();
        move || {
            info!("received signal, shutting down");
            let _ = s.try_send(());
            exit.store(true, Ordering::SeqCst);
        }
    }).map_err(|e| RtlTcpError::Config(format!("could not set signal handler: {e}")))?;

    // Bind master port
    let master_listener = bind_master_port(&args)?;

    // Bind slave port
    let slave_port = args.slave_port.unwrap();
    let slave_listener = TcpListener::bind(format!("{}:{}", args.address, slave_port))?;
    info!("slave port listening on {slave_port}");

    // Accept master
    info!("waiting for master connection on port {}…", args.master_port);
    let (master_stream, addr) = master_listener.accept()?;
    let client_ip = addr.ip().to_canonical().to_string();
    check_whitelist(&client_ip, &args.whitelist)
        .map_err(|e| { warn!("Connection from {client_ip} refused"); e })?;
    info!("master connected from {addr}");
    master_stream.set_read_timeout(Some(read_timeout))?;
    master_stream.set_write_timeout(Some(Duration::from_secs(args.write_timeout)))?;

    all_streams.lock().unwrap().push(master_stream.try_clone()?);

    // Send magic packet via dedicated write clone
    let mut bufw = BufWriter::with_capacity(args.tcp_buffers, master_stream.try_clone()?);
    bufw.write_all(magic_packet)?;
    bufw.flush()?;

    // Start master control thread
    let unknown_count = Arc::new(Mutex::new(0u64));
    let thread_ctl = spawn_master_control_thread(
        master_stream.try_clone()?, ctl.clone(), agc_state.clone(),
        unknown_count, should_exit.clone(), read_timeout,
    );

    // Start slave acceptor thread
    spawn_slave_acceptor(
        slave_listener, tx.clone(), magic_packet.to_vec(),
        args.whitelist.clone(), args.max_slaves, args.tcp_buffers,
        should_exit.clone(), all_streams.clone(),
    );

    // Cancel thread
    let thread_cancel = spawn_cancel_thread(ctl.clone(), receiver, should_exit.clone());

    // USB read callback → broadcast
    let btx = tx.clone();
    let s = sender.clone();
    let _ = reader.read_async(args.buffers, 0, move |bytes| {
        if btx.send(bytes.to_vec()).is_err() {
            let _ = s.try_send(());
        }
    });

    let _ = sender.try_send(());
    let _ = thread_cancel.join();
    let _ = thread_ctl.join();

    // Close all slave streams for graceful shutdown
    {
        let streams = all_streams.lock().unwrap();
        for s in streams.iter() { let _ = s.shutdown(Shutdown::Both); }
    }

    info!("multi-client serve shut down");
    Ok(())
}

/// Spawn the master command-reading control thread
fn spawn_master_control_thread(
    stream: TcpStream,
    ctl: Arc<Mutex<rtlsdr_mt::Controller>>,
    agc_state: Arc<AgcState>,
    unknown_command_count: Arc<Mutex<u64>>,
    should_exit: Arc<AtomicBool>,
    read_timeout: Duration,
) -> thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut stream = stream;
        stream.set_read_timeout(Some(read_timeout)).ok();
        let mut buf = [0u8; COMMAND_HEADER_SIZE];
        let mut rate_limiter = RateLimiter::new(COMMAND_RATE_LIMIT_INTERVAL);
        loop {
            match stream.read_exact(&mut buf) {
                Ok(()) => {}
                Err(e)
                    if e.kind() == ErrorKind::UnexpectedEof
                        || e.kind() == ErrorKind::ConnectionReset
                        || e.kind() == ErrorKind::BrokenPipe
                        || e.kind() == ErrorKind::TimedOut
                        || e.kind() == ErrorKind::ConnectionAborted
                        || e.kind() == ErrorKind::NotConnected =>
                {
                    info!("client disconnected: {e}");
                    break;
                }
                Err(e) => {
                    warn!("read error from client: {e}");
                    break;
                }
            }

            if should_exit.load(Ordering::SeqCst) {
                info!("exit flag set, stopping control thread");
                break;
            }

            // Rate limiting check
            if !rate_limiter.check() {
                debug!("command rate limited, skipping");
                continue;
            }

            let cmd = buf[0];
            let payload: [u8; 4] = [buf[1], buf[2], buf[3], buf[4]];

            match cmd {
                CMD_SET_FREQUENCY => {
                    let freq = u32::from_be_bytes(payload);
                    if let Err(e) = validate_frequency(freq) {
                        warn!("invalid frequency: {e}");
                        continue;
                    }
                    info!("setting center freq to {freq}");
                    with_control(&ctl, |guard| {
                        if let Err(e) = guard.set_center_freq(freq) {
                            warn!("failed to set center freq: {e:?}");
                        }
                    });
                }
                CMD_SET_SAMPLE_RATE => {
                    let sample_rate = u32::from_be_bytes(payload);
                    if let Err(e) = validate_sample_rate(sample_rate) {
                        warn!("invalid sample rate: {e}");
                        continue;
                    }
                    info!("setting sample rate to {sample_rate}");
                    with_control(&ctl, |guard| {
                        if let Err(e) = guard.set_sample_rate(sample_rate) {
                            warn!("failed to set sample rate: {e:?}");
                        }
                    });
                }
                CMD_SET_GAIN_MODE => {
                    let gain_mode = i32::from_be_bytes(payload);
                    if gain_mode > 0 {
                        if agc_state.disable() {
                            info!("gain mode set to manual (AGC off)");
                        }
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.disable_agc() {
                                warn!("failed to disable AGC: {e:?}");
                            }
                        });
                    } else {
                        if agc_state.enable() {
                            info!("gain mode set to automatic (AGC on)");
                        }
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.enable_agc() {
                                warn!("failed to enable AGC: {e:?}");
                            }
                        });
                    }
                }
                CMD_SET_TUNER_GAIN => {
                    let gain = i32::from_be_bytes(payload);
                    if let Err(e) = validate_tuner_gain(gain) {
                        warn!("invalid tuner gain: {e}");
                        continue;
                    }
                    info!("setting manual gain to {gain}");
                    with_control(&ctl, |guard| {
                        if let Err(e) = guard.set_tuner_gain(gain) {
                            warn!("failed to set tuner gain: {e:?}");
                        }
                    });
                }
                CMD_SET_PPM => {
                    let ppm = i32::from_be_bytes(payload);
                    if let Err(e) = validate_ppm(ppm) {
                        warn!("invalid ppm: {e}");
                        continue;
                    }
                    info!("setting ppm to {ppm}");
                    with_control(&ctl, |guard| {
                        if let Err(e) = guard.set_ppm(ppm) {
                            warn!("failed to set ppm: {e:?}");
                        }
                    });
                }
                CMD_SET_AGC => {
                    let agc = u32::from_be_bytes(payload) == 1u32;
                    if agc {
                        if agc_state.enable() {
                            info!("setting automatic gain control to on");
                        }
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.enable_agc() {
                                warn!("failed to enable AGC: {e:?}");
                            }
                        });
                    } else {
                        if agc_state.disable() {
                            info!("setting automatic gain control to off");
                        }
                        with_control(&ctl, |guard| {
                            if let Err(e) = guard.disable_agc() {
                                warn!("failed to disable AGC: {e:?}");
                            }
                        });
                    }
                }
                CMD_CHAIN_DETECT => {
                    info!("chain detection probe from downstream proxy");
                    if let Err(e) = stream.write_all(&[CMD_CHAIN_DETECT, 0x00, 0x00, 0x00, 0x00]) {
                        warn!("failed to send chain detect ack: {e}");
                    }
                }
                _ => {
                    let mut count = unknown_command_count.lock().unwrap();
                    *count += 1;
                    warn!("recv unsupported command {buf:?} (total unknown commands: {count})");
                }
            }
        }
        info!("control thread exiting");
    })
}

/// Spawn the slave acceptor thread with non-blocking accept loop
fn spawn_slave_acceptor(
    listener: TcpListener, tx: broadcast::Sender<Vec<u8>>, magic: Vec<u8>,
    whitelist: Vec<String>, max_slaves: usize, tcp_buffers: usize,
    should_exit: Arc<AtomicBool>, all_streams: Arc<Mutex<Vec<TcpStream>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        let mut count = 0usize;
        loop {
            if should_exit.load(Ordering::SeqCst) { break; }
            if count >= max_slaves {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            match listener.accept() {
                Ok((stream, addr)) => {
                    let ip = addr.ip().to_canonical().to_string();
                    if let Err(e) = check_whitelist(&ip, &whitelist) {
                        warn!("slave {ip} refused: {e}"); continue;
                    }
                    info!("slave connected from {addr}");
                    stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
                    all_streams.lock().unwrap().push(stream.try_clone().unwrap());
                    let mut bw = BufWriter::with_capacity(tcp_buffers, stream.try_clone().unwrap());
                    let _ = bw.write_all(&magic);
                    let _ = bw.flush();
                    let rx = tx.subscribe();
                    let exit = should_exit.clone();
                    // stream is the original TCP stream (not a clone) — the previous try_clone() calls
                    // produced independent FDs that are now owned by all_streams and the dropped BufWriter
                    thread::spawn(move || stream::write_client_loop(stream, rx, &exit));
                    count += 1;
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => { warn!("slave accept error: {e}"); thread::sleep(Duration::from_millis(100)); }
            }
        }
    })
}

/// Spawn the cancel thread that listens for shutdown signal and cancels async reads
fn spawn_cancel_thread(
    ctl: Arc<Mutex<rtlsdr_mt::Controller>>,
    receiver: std::sync::mpsc::Receiver<()>,
    should_exit: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = receiver.recv();
        info!("stopping read from device");
        if let Ok(mut guard) = ctl.lock() {
            guard.cancel_async_read();
        }
        should_exit.store(true, Ordering::SeqCst);
    })
}
