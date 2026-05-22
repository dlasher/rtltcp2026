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
use crate::error::RtlTcpError;

/// RTL-TCP protocol command codes
const COMMAND_HEADER_SIZE: usize = 5;
const CMD_SET_FREQUENCY: u8 = 0x01;
const CMD_SET_SAMPLE_RATE: u8 = 0x02;
const CMD_SET_GAIN_MODE: u8 = 0x03;
const CMD_SET_TUNER_GAIN: u8 = 0x04;
const CMD_SET_PPM: u8 = 0x05;
const CMD_SET_AGC: u8 = 0x08;
const CMD_CHAIN_DETECT: u8 = 0xF0;

/// Magic packet sent to client on connect:
/// "RTL0" (4 bytes) + tuner type 5 (4 bytes BE) + max gain value 0x1d (4 bytes BE)
const MAGIC_PACKET: &[u8] = b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d";

/// Valid frequency range for RTL-SDR devices (0 Hz to 2.2 GHz)
const FREQ_MIN: u32 = 0;
const FREQ_MAX: u32 = 2_200_000_000;
const FREQ_RANGE: RangeInclusive<u32> = FREQ_MIN..=FREQ_MAX;

/// Valid sample rate range (0 Hz to 3.2 MHz)
const SAMPLE_RATE_MIN: u32 = 0;
const SAMPLE_RATE_MAX: u32 = 3_200_000;
const SAMPLE_RATE_RANGE: RangeInclusive<u32> = SAMPLE_RATE_MIN..=SAMPLE_RATE_MAX;

/// Valid PPM correction range (-200 to 200)
const PPM_MIN: i32 = -200;
const PPM_MAX: i32 = 200;
const PPM_RANGE: RangeInclusive<i32> = PPM_MIN..=PPM_MAX;

/// Valid tuner gain range (0 to 500, representing 0 to 50 dB in 0.1 dB steps)
const TUNER_GAIN_MIN: i32 = 0;
const TUNER_GAIN_MAX: i32 = 500;
const TUNER_GAIN_RANGE: RangeInclusive<i32> = TUNER_GAIN_MIN..=TUNER_GAIN_MAX;

/// Minimum interval between commands to prevent flooding (50 ms)
const COMMAND_RATE_LIMIT_INTERVAL: Duration = Duration::from_millis(50);

/// Execute an operation on the device control handle, handling mutex poisoning gracefully.
fn with_control<T, F>(ctl: &Mutex<T>, op: F)
where
    F: FnOnce(&mut T),
{
    match ctl.lock() {
        Ok(mut guard) => {
            op(&mut *guard);
        }
        Err(_) => {
            warn!("mutex poisoned in control thread");
        }
    }
}

/// Validate a frequency value, returning an error message if out of bounds.
fn validate_frequency(freq: u32) -> Result<(), String> {
    if !FREQ_RANGE.contains(&freq) {
        Err(format!(
            "frequency {freq} Hz out of range ({FREQ_MIN}-{FREQ_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Validate a sample rate value, returning an error message if out of bounds.
fn validate_sample_rate(rate: u32) -> Result<(), String> {
    if !SAMPLE_RATE_RANGE.contains(&rate) {
        Err(format!(
            "sample rate {rate} Hz out of range ({SAMPLE_RATE_MIN}-{SAMPLE_RATE_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Validate a PPM correction value, returning an error message if out of bounds.
fn validate_ppm(ppm: i32) -> Result<(), String> {
    if !PPM_RANGE.contains(&ppm) {
        Err(format!("ppm {ppm} out of range ({PPM_MIN}-{PPM_MAX})"))
    } else {
        Ok(())
    }
}

/// Validate a tuner gain value, returning an error message if out of bounds.
fn validate_tuner_gain(gain: i32) -> Result<(), String> {
    if !TUNER_GAIN_RANGE.contains(&gain) {
        Err(format!(
            "tuner gain {gain} out of range ({TUNER_GAIN_MIN}-{TUNER_GAIN_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Check if an IP address passes the whitelist, returning Ok if allowed, Err if rejected.
fn check_whitelist(client_ip: &str, whitelist: &[String]) -> StdResult<(), RtlTcpError> {
    if whitelist.is_empty() {
        return Ok(());
    }
    if is_ip_in_whitelist(client_ip, whitelist) {
        Ok(())
    } else {
        Err(RtlTcpError::Network(
            "connection rejected: IP not in whitelist".to_string(),
        ))
    }
}

/// Check if an IP address is in the whitelist
fn is_ip_in_whitelist(client_ip: &str, whitelist: &[String]) -> bool {
    // Parse the client IP, mapping IPv4-mapped IPv6 addresses to IPv4
    let client_ip: IpAddr = match client_ip.parse::<IpAddr>() {
        Ok(ip) => ip.to_canonical(),
        Err(_) => {
            warn!(target: "rtltcp", "Invalid client IP: {}", client_ip);
            return false;
        }
    };

    // Check if any whitelist entry contains the IP
    for cidr in whitelist {
        match cidr.parse::<IpNet>() {
            Ok(network) => {
                if network.contains(&client_ip) {
                    return true;
                }
            }
            Err(e) => {
                warn!(target: "rtltcp", "Invalid CIDR in whitelist: {} - {}", cidr, e);
            }
        }
    }

    false
}

/// Tracks whether AGC is enabled and detects state changes to suppress redundant log messages.
struct AgcState {
    enabled: AtomicBool,
}

impl AgcState {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
        }
    }

    fn enable(&self) -> bool {
        !self.enabled.swap(true, Ordering::SeqCst)
    }

    fn disable(&self) -> bool {
        self.enabled.swap(false, Ordering::SeqCst)
    }
}

/// Simple rate limiter that tracks the time of the last allowed command.
struct RateLimiter {
    last_command: Instant,
    min_interval: Duration,
}

impl RateLimiter {
    fn new(min_interval: Duration) -> Self {
        Self {
            last_command: Instant::now()
                .checked_sub(min_interval)
                .unwrap_or(Instant::now()),
            min_interval,
        }
    }

    /// Check if a command is allowed under the rate limit.
    /// Returns true if allowed, false if the command should be rejected.
    fn check(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_command);
        if elapsed >= self.min_interval {
            self.last_command = now;
            true
        } else {
            false
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_packet_length() {
        assert_eq!(MAGIC_PACKET.len(), 12);
    }

    #[test]
    fn test_command_constants() {
        assert_eq!(CMD_SET_FREQUENCY, 0x01);
        assert_eq!(CMD_SET_SAMPLE_RATE, 0x02);
        assert_eq!(CMD_SET_GAIN_MODE, 0x03);
        assert_eq!(CMD_SET_TUNER_GAIN, 0x04);
        assert_eq!(CMD_SET_PPM, 0x05);
        assert_eq!(CMD_SET_AGC, 0x08);
    }

    #[test]
    fn test_command_header_size() {
        assert_eq!(COMMAND_HEADER_SIZE, 5);
    }

    #[test]
    fn test_magic_packet_content() {
        // "RTL0" + 5 (tuner type) + 0x1d (max gain)
        assert_eq!(&MAGIC_PACKET[0..4], b"RTL0");
        assert_eq!(&MAGIC_PACKET[4..8], &5u32.to_be_bytes());
        assert_eq!(&MAGIC_PACKET[8..12], &0x1du32.to_be_bytes());
    }

    #[test]
    fn test_parse_frequency_command() {
        // Command 0x01 with frequency 100.5 MHz (0x05FD8220 in big-endian)
        let buf: [u8; 5] = [0x01, 0x05, 0xFD, 0x82, 0x20];
        assert_eq!(buf[0], CMD_SET_FREQUENCY);
        let freq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(freq, 100_500_000);
    }

    #[test]
    fn test_parse_sample_rate_command() {
        // Command 0x02 with sample rate 2.048 MS/s (0x001F4000 in big-endian)
        let buf: [u8; 5] = [0x02, 0x00, 0x1F, 0x40, 0x00];
        assert_eq!(buf[0], CMD_SET_SAMPLE_RATE);
        let sample_rate = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(sample_rate, 2_048_000);
    }

    #[test]
    fn test_parse_gain_mode_command() {
        // Command 0x03 with gain_mode=1 (manual)
        let buf: [u8; 5] = [0x03, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(buf[0], CMD_SET_GAIN_MODE);
        let gain_mode = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(gain_mode, 1);
    }

    #[test]
    fn test_parse_tuner_gain_command() {
        // Command 0x04 with tuner gain 30 (0x0000001E in big-endian)
        let buf: [u8; 5] = [0x04, 0x00, 0x00, 0x00, 0x1E];
        assert_eq!(buf[0], CMD_SET_TUNER_GAIN);
        let gain = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(gain, 30);
    }

    #[test]
    fn test_parse_ppm_command() {
        // Command 0x05 with PPM error of 50 (0x00000032 in big-endian)
        let buf: [u8; 5] = [0x05, 0x00, 0x00, 0x00, 0x32];
        assert_eq!(buf[0], CMD_SET_PPM);
        let ppm = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(ppm, 50);
    }

    #[test]
    fn test_parse_agc_command() {
        // Command 0x08 with AGC=1 (on)
        let buf: [u8; 5] = [0x08, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(buf[0], CMD_SET_AGC);
        let agc = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(agc, 1);
    }

    #[test]
    fn test_unknown_command_zero() {
        // Command byte 0x00 is unknown/unsupported
        let buf: [u8; 5] = [0x00, 0x00, 0x00, 0x00, 0x00];
        assert_ne!(buf[0], CMD_SET_FREQUENCY);
        assert_ne!(buf[0], CMD_SET_SAMPLE_RATE);
        assert_ne!(buf[0], CMD_SET_GAIN_MODE);
        assert_ne!(buf[0], CMD_SET_TUNER_GAIN);
        assert_ne!(buf[0], CMD_SET_PPM);
        assert_ne!(buf[0], CMD_SET_AGC);
    }

    #[test]
    fn test_unknown_command_ff() {
        // Command byte 0xFF is unknown/unsupported
        let buf: [u8; 5] = [0xFF, 0x00, 0x00, 0x00, 0x00];
        assert_ne!(buf[0], CMD_SET_FREQUENCY);
        assert_ne!(buf[0], CMD_SET_SAMPLE_RATE);
        assert_ne!(buf[0], CMD_SET_GAIN_MODE);
        assert_ne!(buf[0], CMD_SET_TUNER_GAIN);
        assert_ne!(buf[0], CMD_SET_PPM);
        assert_ne!(buf[0], CMD_SET_AGC);
    }

    // --- Input validation tests (1.2) ---

    #[test]
    fn test_validate_frequency_valid() {
        assert!(validate_frequency(0).is_ok());
        assert!(validate_frequency(100_000_000).is_ok());
        assert!(validate_frequency(2_200_000_000).is_ok());
    }

    #[test]
    fn test_validate_frequency_invalid() {
        // Frequencies above max (but since u32 wraps, we just check the max boundary)
        // u32::MAX is above FREQ_MAX
        assert!(validate_frequency(FREQ_MAX + 1).is_err());
        assert!(validate_frequency(u32::MAX).is_err());
    }

    #[test]
    fn test_validate_sample_rate_valid() {
        assert!(validate_sample_rate(0).is_ok());
        assert!(validate_sample_rate(2_048_000).is_ok());
        assert!(validate_sample_rate(3_200_000).is_ok());
    }

    #[test]
    fn test_validate_sample_rate_invalid() {
        assert!(validate_sample_rate(SAMPLE_RATE_MAX + 1).is_err());
        assert!(validate_sample_rate(u32::MAX).is_err());
    }

    #[test]
    fn test_validate_ppm_valid() {
        assert!(validate_ppm(0).is_ok());
        assert!(validate_ppm(-200).is_ok());
        assert!(validate_ppm(200).is_ok());
        assert!(validate_ppm(50).is_ok());
        assert!(validate_ppm(-100).is_ok());
    }

    #[test]
    fn test_validate_ppm_invalid() {
        assert!(validate_ppm(PPM_MIN - 1).is_err());
        assert!(validate_ppm(PPM_MAX + 1).is_err());
        assert!(validate_ppm(-300).is_err());
        assert!(validate_ppm(300).is_err());
    }

    #[test]
    fn test_validate_tuner_gain_valid() {
        assert!(validate_tuner_gain(0).is_ok());
        assert!(validate_tuner_gain(30).is_ok());
        assert!(validate_tuner_gain(500).is_ok());
    }

    #[test]
    fn test_validate_tuner_gain_invalid() {
        assert!(validate_tuner_gain(TUNER_GAIN_MIN - 1).is_err());
        assert!(validate_tuner_gain(TUNER_GAIN_MAX + 1).is_err());
        assert!(validate_tuner_gain(-10).is_err());
        assert!(validate_tuner_gain(600).is_err());
    }

    // --- Rate limiter tests (1.3) ---

    #[test]
    fn test_rate_limiter_allows_after_interval() {
        let mut limiter = RateLimiter::new(Duration::from_millis(10));
        // First command should always be allowed (last_command set to before now)
        assert!(limiter.check());
    }

    #[test]
    fn test_rate_limiter_blocks_rapid_commands() {
        let mut limiter = RateLimiter::new(Duration::from_millis(500));
        // Consume the initial allowance
        limiter.last_command = Instant::now();
        // Immediate next call should be denied
        assert!(!limiter.check());
    }

    #[test]
    fn test_rate_limiter_allows_after_sleep() {
        let mut limiter = RateLimiter::new(Duration::from_millis(10));
        limiter.last_command = Instant::now();
        std::thread::sleep(Duration::from_millis(15));
        assert!(limiter.check());
    }

    // --- AgcState tests ---

    #[test]
    fn test_agc_state_initial_true() {
        let agc = AgcState::new();
        assert!(agc.enabled.load(Ordering::SeqCst));
    }

    #[test]
    fn test_agc_state_enable_unchanged() {
        let agc = AgcState::new();
        assert!(!agc.enable());
    }

    #[test]
    fn test_agc_state_disable_changed() {
        let agc = AgcState::new();
        assert!(agc.disable());
    }

    #[test]
    fn test_agc_state_enable_after_disable() {
        let agc = AgcState::new();
        agc.disable();
        assert!(agc.enable());
    }

    #[test]
    fn test_agc_state_disable_twice_unchanged() {
        let agc = AgcState::new();
        agc.disable();
        assert!(!agc.disable());
    }

    // --- check_whitelist tests ---

    #[test]
    fn test_check_whitelist_matching_ip_allows() {
        let whitelist = vec!["192.168.1.0/24".to_string()];
        let result = check_whitelist("192.168.1.50", &whitelist);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_whitelist_non_matching_rejects() {
        let whitelist = vec!["192.168.1.0/24".to_string()];
        let result = check_whitelist("10.0.0.1", &whitelist);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RtlTcpError::Network(_)));
    }

    #[test]
    fn test_check_whitelist_empty_allows() {
        let whitelist: Vec<String> = vec![];
        let result = check_whitelist("10.0.0.1", &whitelist);
        assert!(result.is_ok());
    }

    // --- IP whitelist tests ---

    #[test]
    fn test_whitelist_ipv4_single() {
        let whitelist = vec!["192.168.1.100/32".to_string()];
        assert!(is_ip_in_whitelist("192.168.1.100", &whitelist));
        assert!(!is_ip_in_whitelist("192.168.1.101", &whitelist));
        assert!(!is_ip_in_whitelist("192.168.2.100", &whitelist));
    }

    #[test]
    fn test_whitelist_ipv4_cidr() {
        let whitelist = vec!["192.168.100.0/24".to_string()];
        assert!(is_ip_in_whitelist("192.168.100.1", &whitelist));
        assert!(is_ip_in_whitelist("192.168.100.255", &whitelist));
        assert!(!is_ip_in_whitelist("192.168.101.1", &whitelist));
        assert!(!is_ip_in_whitelist("10.0.0.1", &whitelist));
    }

    #[test]
    fn test_whitelist_ipv4_cidr_16() {
        let whitelist = vec!["10.0.0.0/16".to_string()];
        assert!(is_ip_in_whitelist("10.0.0.1", &whitelist));
        assert!(is_ip_in_whitelist("10.0.255.255", &whitelist));
        assert!(!is_ip_in_whitelist("10.1.0.1", &whitelist));
    }

    #[test]
    fn test_whitelist_multiple_entries() {
        let whitelist = vec!["192.168.1.0/24".to_string(), "10.0.0.0/8".to_string()];
        assert!(is_ip_in_whitelist("192.168.1.50", &whitelist));
        assert!(is_ip_in_whitelist("10.50.100.200", &whitelist));
        assert!(!is_ip_in_whitelist("172.16.0.1", &whitelist));
    }

    #[test]
    fn test_whitelist_empty() {
        let whitelist: Vec<String> = vec![];
        // Empty whitelist should allow all IPs (not deny all)
        // The calling code checks whitelist.is_empty() first
        assert!(!is_ip_in_whitelist("192.168.1.1", &whitelist));
    }

    #[test]
    fn test_whitelist_invalid_cidr() {
        let whitelist = vec!["invalid-cidr".to_string()];
        // Invalid CIDR should result in IP not being in whitelist
        assert!(!is_ip_in_whitelist("192.168.1.1", &whitelist));
    }

    #[test]
    fn test_whitelist_ipv6() {
        let whitelist = vec!["::1/128".to_string()];
        assert!(is_ip_in_whitelist("::1", &whitelist));
        assert!(!is_ip_in_whitelist("::2", &whitelist));
    }

    #[test]
    fn test_whitelist_ipv4_mapped() {
        let whitelist = vec!["10.4.10.0/24".to_string()];
        // IPv4-mapped IPv6 addresses should be normalized and match IPv4 CIDRs
        assert!(is_ip_in_whitelist("::ffff:10.4.10.71", &whitelist));
        assert!(is_ip_in_whitelist("::ffff:10.4.10.1", &whitelist));
        assert!(!is_ip_in_whitelist("::ffff:10.4.11.1", &whitelist));
        assert!(!is_ip_in_whitelist("::ffff:192.168.1.1", &whitelist));
    }

    #[test]
    fn test_chain_detect_ack_response() {
        let buf: [u8; 5] = [0xF0, 0x50, 0x52, 0x4F, 0x58];
        assert_eq!(buf[0], CMD_CHAIN_DETECT);
        let magic = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(magic, 0x50524F58);
    }

    #[test]
    fn test_chain_detect_ack_format() {
        let ack: [u8; 5] = [0xF0, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(ack.len(), COMMAND_HEADER_SIZE);
        assert_eq!(ack[0], CMD_CHAIN_DETECT);
    }
}
