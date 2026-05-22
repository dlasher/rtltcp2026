use std::io::prelude::*;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::net::{IpAddr, Shutdown, TcpListener};
use std::ops::RangeInclusive;
use std::result::Result as StdResult;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use ipnet::IpNet;
#[cfg(feature = "systemd")]
use listenfd::ListenFd;
use tracing::{debug, info, warn};

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

    // Validate buffers and tcp_buffers manually
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

    let listener;
    #[cfg(feature = "systemd")]
    {
        let mut listenfd = ListenFd::from_env();
        listener = if let Some(listener) = listenfd.take_tcp_listener(0).map_err(|e| {
            RtlTcpError::Config(format!(
                "could not get file descriptor from environment: {e}"
            ))
        })? {
            listener
        } else {
            TcpListener::bind(format!("{}:{}", args.address, args.master_port))?
        };
        systemd::daemon::notify(false, [(systemd::daemon::STATE_READY, "1")].iter())?;
    }
    #[cfg(not(feature = "systemd"))]
    {
        listener = TcpListener::bind(format!("{}:{}", args.address, args.master_port))?;
    }

    let (sender, receiver) = sync_channel(1);
    let sender_ctrlc = sender.clone();
    let should_exit = Arc::new(AtomicBool::new(false));
    let should_exit_ctrlc = should_exit.clone();
    let agc_state = Arc::new(AgcState::new());

    let read_timeout = Duration::from_secs(args.read_timeout);
    let write_timeout = Duration::from_secs(args.write_timeout);

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
