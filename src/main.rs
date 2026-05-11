use std::io::prelude::*;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::net::{IpAddr, TcpListener, Shutdown};
use std::result::Result as StdResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
#[cfg(feature = "systemd")]
use listenfd::ListenFd;
use tracing::{debug, info, warn};
use ipnet::IpNet;

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

/// Magic packet sent to client on connect:
/// "RTL0" (4 bytes) + tuner type 5 (4 bytes BE) + max gain value 0x1d (4 bytes BE)
const MAGIC_PACKET: &[u8] = b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d";

/// Valid frequency range for RTL-SDR devices (0 Hz to 2.2 GHz)
const FREQ_MIN: u32 = 0;
const FREQ_MAX: u32 = 2_200_000_000;

/// Valid sample rate range (0 Hz to 3.2 MHz)
const SAMPLE_RATE_MIN: u32 = 0;
const SAMPLE_RATE_MAX: u32 = 3_200_000;

/// Valid PPM correction range (-200 to 200)
const PPM_MIN: i32 = -200;
const PPM_MAX: i32 = 200;

/// Valid tuner gain range (0 to 500, representing 0 to 50 dB in 0.1 dB steps)
const TUNER_GAIN_MIN: i32 = 0;
const TUNER_GAIN_MAX: i32 = 500;

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
    if freq < FREQ_MIN || freq > FREQ_MAX {
        Err(format!("frequency {freq} Hz out of range ({FREQ_MIN}-{FREQ_MAX})"))
    } else {
        Ok(())
    }
}

/// Validate a sample rate value, returning an error message if out of bounds.
fn validate_sample_rate(rate: u32) -> Result<(), String> {
    if rate < SAMPLE_RATE_MIN || rate > SAMPLE_RATE_MAX {
        Err(format!(
            "sample rate {rate} Hz out of range ({SAMPLE_RATE_MIN}-{SAMPLE_RATE_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Validate a PPM correction value, returning an error message if out of bounds.
fn validate_ppm(ppm: i32) -> Result<(), String> {
    if ppm < PPM_MIN || ppm > PPM_MAX {
        Err(format!("ppm {ppm} out of range ({PPM_MIN}-{PPM_MAX})"))
    } else {
        Ok(())
    }
}

/// Validate a tuner gain value, returning an error message if out of bounds.
fn validate_tuner_gain(gain: i32) -> Result<(), String> {
    if gain < TUNER_GAIN_MIN || gain > TUNER_GAIN_MAX {
        Err(format!(
            "tuner gain {gain} out of range ({TUNER_GAIN_MIN}-{TUNER_GAIN_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Check if an IP address is in the whitelist
fn is_ip_in_whitelist(client_ip: &str, whitelist: &[String]) -> bool {
    // Parse the client IP
    let client_ip: IpAddr = match client_ip.parse() {
        Ok(ip) => ip,
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
                .unwrap_or_else(|| Instant::now()),
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
    /// listen address
    #[clap(short, long, default_value = "127.0.0.1")]
    address: String,

    /// listen port
    #[clap(short, long, default_value_t = 1234)]
    port: u16,

    /// device index
    #[clap(short, long, default_value_t = 0)]
    device_index: u32,

    /// number of decoding buffers
    #[clap(short, long, default_value_t = 15)]
    buffers: u32,

    /// tcp sending buffer size (in bytes)
    #[clap(short = 's', long, default_value_t = 512000)]
    tcp_buffers: usize,

    /// socket read timeout in seconds
    #[clap(long, default_value_t = 30)]
    read_timeout: u64,

    /// socket write timeout in seconds
    #[clap(long, default_value_t = 30)]
    write_timeout: u64,

    /// IP whitelist (CIDR notation), e.g. 192.168.100.0/24 (can be specified multiple times)
    #[clap(long)]
    whitelist: Vec<String>,
}

fn main() -> StdResult<(), RtlTcpError> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Validate buffers and tcp_buffers manually
    if args.buffers == 0 || args.buffers > 32 {
        return Err(RtlTcpError::ConfigError("buffers must be between 1 and 32".to_string()));
    }
    if args.tcp_buffers == 0 || args.tcp_buffers > 10_485_760 {
        return Err(RtlTcpError::ConfigError("tcp_buffers must be between 1 and 10485760 (10MB)".to_string()));
    }
    if args.read_timeout == 0 {
        return Err(RtlTcpError::ConfigError("read_timeout must be greater than 0".to_string()));
    }
    if args.write_timeout == 0 {
        return Err(RtlTcpError::ConfigError("write_timeout must be greater than 0".to_string()));
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
        listener = if let Some(listener) = listenfd
            .take_tcp_listener(0)
            .map_err(|e| RtlTcpError::ConfigError(format!("could not get file descriptor from environment: {e}")))?
        {
            listener
        } else {
            TcpListener::bind(format!("{}:{}", args.address, args.port))?
        };
        systemd::daemon::notify(false, [(systemd::daemon::STATE_READY, "1")].iter())?;
    }
    #[cfg(not(feature = "systemd"))]
    {
        listener = TcpListener::bind(format!("{}:{}", args.address, args.port))?;
    }

    let (sender, receiver) = sync_channel(1);
    let sender_ctrlc = sender.clone();
    let should_exit = Arc::new(AtomicBool::new(false));
    let should_exit_ctrlc = should_exit.clone();

    let read_timeout = Duration::from_secs(args.read_timeout);
    let write_timeout = Duration::from_secs(args.write_timeout);

info!("waiting for connection…");
    let (stream, addr) = listener.accept()?;
    
    // Check if the client IP is in the whitelist if one is configured
    let client_ip = match addr {
        std::net::SocketAddr::V4(v4_addr) => v4_addr.ip().to_string(),
        std::net::SocketAddr::V6(v6_addr) => v6_addr.ip().to_string(),
    };
    
    // If whitelist is configured, check the client IP against it
    if !args.whitelist.is_empty() {
        let ip_in_whitelist = is_ip_in_whitelist(&client_ip, &args.whitelist);
        if !ip_in_whitelist {
            info!("Client IP {} is not in whitelist, rejecting connection", client_ip);
            warn!(target: "rtltcp", "Connection from {} refused due to IP not in whitelist", client_ip);
            return Ok(());
        }
    }
    
    info!("connection from {addr}");
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(write_timeout))?;

    // Task 2.2: Clone the stream for signal-based shutdown.
    // This allows the signal handler to interrupt blocking reads immediately,
    // rather than waiting for the read timeout to expire.
    let stream_for_shutdown = Arc::new(Mutex::new(Some(stream.try_clone()?)));
    let stream_for_shutdown_ctrlc = stream_for_shutdown.clone();

    let (ctl, mut reader) =
        rtlsdr_mt::open(args.device_index).map_err(|e| RtlTcpError::DeviceError(format!("could not open RTL-SDR device: {e:?}")))?;
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
    }).map_err(|e| RtlTcpError::ConfigError(format!("could not set signal handler: {e}")))?;

    // Task 2.3: Track unknown commands for better visibility
    let unknown_command_count = Arc::new(Mutex::new(0u64));

    let thread_ctl = std::thread::spawn({
        let ctl = ctl.clone();
        let should_exit = should_exit.clone();
        let unknown_command_count = unknown_command_count.clone();
        let mut stream = stream.try_clone()?;
        move || {
            let mut buf = [0u8; COMMAND_HEADER_SIZE];
            let mut rate_limiter = RateLimiter::new(COMMAND_RATE_LIMIT_INTERVAL);
            loop {
                match stream.read_exact(&mut buf) {
                    Ok(()) => {}
                    Err(e) if e.kind() == ErrorKind::UnexpectedEof
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
                            info!("gain mode set to manual (AGC off)");
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.disable_agc() {
                                    warn!("failed to disable AGC: {e:?}");
                                }
                            });
                        } else {
                            info!("gain mode set to automatic (AGC on)");
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
                            info!("setting automatic gain control to on");
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.enable_agc() {
                                    warn!("failed to enable AGC: {e:?}");
                                }
                            });
                        } else {
                            info!("setting automatic gain control to off");
                            with_control(&ctl, |guard| {
                                if let Err(e) = guard.disable_agc() {
                                    warn!("failed to disable AGC: {e:?}");
                                }
                            });
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

    let read_result = reader.read_async(args.buffers, 0, |bytes| {
        if let Err(e) = buf_write_stream.write_all(bytes) {
            warn!("stream write failed, triggering shutdown: {e}");
            let _ = sender.try_send(());
        }
    });

    // Signal cancel thread so it doesn't hang on recv() if read_async completes normally
    let _ = sender.try_send(());

    if let Err(e) = read_result {
        warn!("read_async error: {e:?}");
    }

    // Flush buffer before shutting down
    if let Err(e) = buf_write_stream.flush() {
        warn!("failed to flush write buffer: {e}");
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
        let whitelist = vec![
            "192.168.1.0/24".to_string(),
            "10.0.0.0/8".to_string(),
        ];
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
}
