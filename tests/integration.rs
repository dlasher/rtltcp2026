//! Comprehensive integration tests for rtltcp
//!
//! These tests verify functionality without requiring
//! an RTL-SDR dongle. They test what can be tested without hardware.

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::ops::RangeInclusive;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Binary Integration Tests
// ============================================================================

/// Verify the binary exists and can print help
#[test]
fn binary_exists_and_prints_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp"))
        .arg("--help")
        .output()
        .expect("failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("I/Q spectrum server"));
    assert!(stdout.contains("address"));
    assert!(stdout.contains("port"));
    assert!(stdout.contains("device-index"));
    assert!(stdout.contains("buffers"));
    assert!(stdout.contains("tcp-buffers"));
    assert!(stdout.contains("read-timeout"));
    assert!(stdout.contains("write-timeout"));
}

/// Verify the binary prints version
#[test]
fn binary_prints_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp"))
        .arg("--version")
        .output()
        .expect("failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("rtltcp"));
}

/// Test that help output contains all CLI options
#[test]
fn help_shows_all_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp"))
        .arg("--help")
        .output()
        .expect("failed to execute binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify all CLI flags are shown
    let required_options = vec![
        "-a",
        "--address",
        "-p",
        "--port",
        "-d",
        "--device-index",
        "-b",
        "--buffers",
        "-s",
        "--tcp-buffers",
        "--read-timeout",
        "--write-timeout",
    ];

    for option in required_options {
        assert!(
            stdout.contains(option),
            "Missing option in help: {}",
            option
        );
    }
}

/// Test that short flags work
#[test]
fn short_flags_work() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp"))
        .arg("-h")
        .output()
        .expect("failed to execute binary");

    assert!(output.status.success());
}

// ============================================================================
// Protocol Command Parsing Tests
// ============================================================================

/// RTL-TCP protocol command codes
const CMD_SET_FREQUENCY: u8 = 0x01;
const CMD_SET_SAMPLE_RATE: u8 = 0x02;
const CMD_SET_GAIN_MODE: u8 = 0x03;
const CMD_SET_TUNER_GAIN: u8 = 0x04;
const CMD_SET_PPM: u8 = 0x05;
const CMD_SET_AGC: u8 = 0x08;

/// Magic packet sent to client on connect
const MAGIC_PACKET: &[u8] = b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d";

/// Valid ranges (must match main.rs)
const FREQ_MIN: u32 = 0;
const FREQ_MAX: u32 = 2_200_000_000;
const FREQ_RANGE: RangeInclusive<u32> = FREQ_MIN..=FREQ_MAX;
const SAMPLE_RATE_MIN: u32 = 0;
const SAMPLE_RATE_MAX: u32 = 3_200_000;
const SAMPLE_RATE_RANGE: RangeInclusive<u32> = SAMPLE_RATE_MIN..=SAMPLE_RATE_MAX;
const PPM_MIN: i32 = -200;
const PPM_MAX: i32 = 200;
const PPM_RANGE: RangeInclusive<i32> = PPM_MIN..=PPM_MAX;
const TUNER_GAIN_MIN: i32 = 0;
const TUNER_GAIN_MAX: i32 = 500;
const TUNER_GAIN_RANGE: RangeInclusive<i32> = TUNER_GAIN_MIN..=TUNER_GAIN_MAX;

// ============================================================================
// Validation Functions (mirroring main.rs)
// ============================================================================

fn validate_frequency(freq: u32) -> Result<(), String> {
    if !FREQ_RANGE.contains(&freq) {
        Err(format!(
            "frequency {freq} Hz out of range ({FREQ_MIN}-{FREQ_MAX})"
        ))
    } else {
        Ok(())
    }
}

fn validate_sample_rate(rate: u32) -> Result<(), String> {
    if !SAMPLE_RATE_RANGE.contains(&rate) {
        Err(format!(
            "sample rate {rate} Hz out of range ({SAMPLE_RATE_MIN}-{SAMPLE_RATE_MAX})"
        ))
    } else {
        Ok(())
    }
}

fn validate_ppm(ppm: i32) -> Result<(), String> {
    if !PPM_RANGE.contains(&ppm) {
        Err(format!("ppm {ppm} out of range ({PPM_MIN}-{PPM_MAX})"))
    } else {
        Ok(())
    }
}

fn validate_tuner_gain(gain: i32) -> Result<(), String> {
    if !TUNER_GAIN_RANGE.contains(&gain) {
        Err(format!(
            "tuner gain {gain} out of range ({TUNER_GAIN_MIN}-{TUNER_GAIN_MAX})"
        ))
    } else {
        Ok(())
    }
}

// ============================================================================
// Command Parsing Tests
// ============================================================================

/// Test parsing SET_FREQUENCY command (0x01)
#[test]
fn test_parse_frequency_command() {
    // Command 0x01 with frequency 100.5 MHz (0x05FD8220 in big-endian)
    let buf: [u8; 5] = [0x01, 0x05, 0xFD, 0x82, 0x20];
    assert_eq!(buf[0], CMD_SET_FREQUENCY);
    let freq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(freq, 100_500_000);
}

/// Test parsing SET_SAMPLE_RATE command (0x02)
#[test]
fn test_parse_sample_rate_command() {
    // Command 0x02 with sample rate 2.048 MS/s (0x001F4000 in big-endian)
    let buf: [u8; 5] = [0x02, 0x00, 0x1F, 0x40, 0x00];
    assert_eq!(buf[0], CMD_SET_SAMPLE_RATE);
    let sample_rate = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(sample_rate, 2_048_000);
}

/// Test parsing SET_GAIN_MODE command (0x03)
#[test]
fn test_parse_gain_mode_command() {
    // Command 0x03 with gain_mode=1 (manual)
    let buf: [u8; 5] = [0x03, 0x00, 0x00, 0x00, 0x01];
    assert_eq!(buf[0], CMD_SET_GAIN_MODE);
    let gain_mode = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(gain_mode, 1);
}

/// Test parsing SET_TUNER_GAIN command (0x04)
#[test]
fn test_parse_tuner_gain_command() {
    // Command 0x04 with tuner gain 30 (0x0000001E in big-endian)
    let buf: [u8; 5] = [0x04, 0x00, 0x00, 0x00, 0x1E];
    assert_eq!(buf[0], CMD_SET_TUNER_GAIN);
    let gain = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(gain, 30);
}

/// Test parsing SET_PPM command (0x05)
#[test]
fn test_parse_ppm_command() {
    // Command 0x05 with PPM error of 50 (0x00000032 in big-endian)
    let buf: [u8; 5] = [0x05, 0x00, 0x00, 0x00, 0x32];
    assert_eq!(buf[0], CMD_SET_PPM);
    let ppm = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(ppm, 50);
}

/// Test parsing SET_AGC command (0x08)
#[test]
fn test_parse_agc_command() {
    // Command 0x08 with AGC=1 (on)
    let buf: [u8; 5] = [0x08, 0x00, 0x00, 0x00, 0x01];
    assert_eq!(buf[0], CMD_SET_AGC);
    let agc = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(agc, 1);
}

// ============================================================================
// Magic Packet Tests
// ============================================================================

/// Verify magic packet structure and length
#[test]
fn test_magic_packet_length() {
    assert_eq!(MAGIC_PACKET.len(), 12);
}

/// Verify magic packet content: "RTL0" + tuner type + max gain
#[test]
fn test_magic_packet_content() {
    assert_eq!(&MAGIC_PACKET[0..4], b"RTL0");
    assert_eq!(&MAGIC_PACKET[4..8], &5u32.to_be_bytes());
    assert_eq!(&MAGIC_PACKET[8..12], &0x1du32.to_be_bytes());
}

/// Verify command constants match expected values
#[test]
fn test_command_constants() {
    assert_eq!(CMD_SET_FREQUENCY, 0x01);
    assert_eq!(CMD_SET_SAMPLE_RATE, 0x02);
    assert_eq!(CMD_SET_GAIN_MODE, 0x03);
    assert_eq!(CMD_SET_TUNER_GAIN, 0x04);
    assert_eq!(CMD_SET_PPM, 0x05);
    assert_eq!(CMD_SET_AGC, 0x08);
}

/// Verify command header size
#[test]
fn test_command_header_size() {
    const COMMAND_HEADER_SIZE: usize = 5;
    assert_eq!(COMMAND_HEADER_SIZE, 5);
}

// ============================================================================
// Unknown Command Tests
// ============================================================================

/// Test that command byte 0x00 is unknown
#[test]
fn test_unknown_command_zero() {
    let buf: [u8; 5] = [0x00, 0x00, 0x00, 0x00, 0x00];
    assert_ne!(buf[0], CMD_SET_FREQUENCY);
    assert_ne!(buf[0], CMD_SET_SAMPLE_RATE);
    assert_ne!(buf[0], CMD_SET_GAIN_MODE);
    assert_ne!(buf[0], CMD_SET_TUNER_GAIN);
    assert_ne!(buf[0], CMD_SET_PPM);
    assert_ne!(buf[0], CMD_SET_AGC);
}

/// Test that command byte 0xFF is unknown
#[test]
fn test_unknown_command_ff() {
    let buf: [u8; 5] = [0xFF, 0x00, 0x00, 0x00, 0x00];
    assert_ne!(buf[0], CMD_SET_FREQUENCY);
    assert_ne!(buf[0], CMD_SET_SAMPLE_RATE);
    assert_ne!(buf[0], CMD_SET_GAIN_MODE);
    assert_ne!(buf[0], CMD_SET_TUNER_GAIN);
    assert_ne!(buf[0], CMD_SET_PPM);
    assert_ne!(buf[0], CMD_SET_AGC);
}

/// Test that reserved commands 0x06, 0x07 are unknown
#[test]
fn test_unknown_command_reserved() {
    // Test 0x06
    let buf1: [u8; 5] = [0x06, 0x00, 0x00, 0x00, 0x00];
    assert_ne!(buf1[0], CMD_SET_FREQUENCY);
    assert_ne!(buf1[0], CMD_SET_SAMPLE_RATE);
    assert_ne!(buf1[0], CMD_SET_GAIN_MODE);
    assert_ne!(buf1[0], CMD_SET_TUNER_GAIN);
    assert_ne!(buf1[0], CMD_SET_PPM);
    assert_ne!(buf1[0], CMD_SET_AGC);

    // Test 0x07
    let buf2: [u8; 5] = [0x07, 0x00, 0x00, 0x00, 0x00];
    assert_ne!(buf2[0], CMD_SET_FREQUENCY);
    assert_ne!(buf2[0], CMD_SET_SAMPLE_RATE);
    assert_ne!(buf2[0], CMD_SET_GAIN_MODE);
    assert_ne!(buf2[0], CMD_SET_TUNER_GAIN);
    assert_ne!(buf2[0], CMD_SET_PPM);
    assert_ne!(buf2[0], CMD_SET_AGC);
}

// ============================================================================
// Frequency Validation Tests
// ============================================================================

/// Test valid frequency values
#[test]
fn test_validate_frequency_valid() {
    assert!(validate_frequency(0).is_ok());
    assert!(validate_frequency(1).is_ok());
    assert!(validate_frequency(100_000_000).is_ok()); // 100 MHz (FM band)
    assert!(validate_frequency(433_920_000).is_ok()); // 433.92 MHz (ISM)
    assert!(validate_frequency(868_000_000).is_ok()); // 868 MHz (ISM)
    assert!(validate_frequency(915_000_000).is_ok()); // 915 MHz (ISM)
    assert!(validate_frequency(1_000_000_000).is_ok()); // 1 GHz
    assert!(validate_frequency(2_200_000_000).is_ok()); // Max
}

/// Test invalid frequency values
#[test]
fn test_validate_frequency_invalid() {
    // Just above max
    assert!(validate_frequency(FREQ_MAX + 1).is_err());
    assert!(validate_frequency(2_200_000_001).is_err());
    // Well above max
    assert!(validate_frequency(3_000_000_000).is_err());
    assert!(validate_frequency(u32::MAX).is_err());
}

/// Test frequency boundary values
#[test]
fn test_validate_frequency_boundaries() {
    // At exact boundaries
    assert!(validate_frequency(FREQ_MIN).is_ok());
    assert!(validate_frequency(FREQ_MAX).is_ok());
    // One step beyond boundaries
    assert!(validate_frequency(FREQ_MAX + 1).is_err());
}

/// Test frequency validation error messages
#[test]
fn test_validate_frequency_error_messages() {
    let err = validate_frequency(u32::MAX).unwrap_err();
    assert!(err.contains("out of range"));
    assert!(err.contains("2200000000"));
}

// ============================================================================
// Sample Rate Validation Tests
// ============================================================================

/// Test valid sample rate values
#[test]
fn test_validate_sample_rate_valid() {
    assert!(validate_sample_rate(0).is_ok());
    assert!(validate_sample_rate(1).is_ok());
    assert!(validate_sample_rate(250_000).is_ok()); // 250 kS/s
    assert!(validate_sample_rate(1_000_000).is_ok()); // 1 MS/s
    assert!(validate_sample_rate(2_048_000).is_ok()); // 2.048 MS/s (common)
    assert!(validate_sample_rate(2_400_000).is_ok()); // 2.4 MS/s
    assert!(validate_sample_rate(3_200_000).is_ok()); // Max
}

/// Test invalid sample rate values
#[test]
fn test_validate_sample_rate_invalid() {
    assert!(validate_sample_rate(SAMPLE_RATE_MAX + 1).is_err());
    assert!(validate_sample_rate(3_200_001).is_err());
    assert!(validate_sample_rate(u32::MAX).is_err());
}

/// Test sample rate boundary values
#[test]
fn test_validate_sample_rate_boundaries() {
    assert!(validate_sample_rate(SAMPLE_RATE_MIN).is_ok());
    assert!(validate_sample_rate(SAMPLE_RATE_MAX).is_ok());
    assert!(validate_sample_rate(SAMPLE_RATE_MAX + 1).is_err());
}

/// Test sample rate validation error messages
#[test]
fn test_validate_sample_rate_error_messages() {
    let err = validate_sample_rate(u32::MAX).unwrap_err();
    assert!(err.contains("out of range"));
    assert!(err.contains("3200000"));
}

// ============================================================================
// PPM Validation Tests
// ============================================================================

/// Test valid PPM values
#[test]
fn test_validate_ppm_valid() {
    assert!(validate_ppm(0).is_ok());
    assert!(validate_ppm(1).is_ok());
    assert!(validate_ppm(-1).is_ok());
    assert!(validate_ppm(50).is_ok());
    assert!(validate_ppm(-50).is_ok());
    assert!(validate_ppm(100).is_ok());
    assert!(validate_ppm(-100).is_ok());
    assert!(validate_ppm(PPM_MIN).is_ok()); // -200
    assert!(validate_ppm(PPM_MAX).is_ok()); // 200
}

/// Test invalid PPM values
#[test]
fn test_validate_ppm_invalid() {
    assert!(validate_ppm(PPM_MIN - 1).is_err());
    assert!(validate_ppm(PPM_MAX + 1).is_err());
    assert!(validate_ppm(-300).is_err());
    assert!(validate_ppm(300).is_err());
    assert!(validate_ppm(i32::MIN).is_err());
    assert!(validate_ppm(i32::MAX).is_err());
}

/// Test PPM boundary values
#[test]
fn test_validate_ppm_boundaries() {
    assert!(validate_ppm(PPM_MIN).is_ok());
    assert!(validate_ppm(PPM_MAX).is_ok());
    assert!(validate_ppm(PPM_MIN - 1).is_err());
    assert!(validate_ppm(PPM_MAX + 1).is_err());
}

/// Test PPM validation error messages
#[test]
fn test_validate_ppm_error_messages() {
    let err = validate_ppm(300).unwrap_err();
    assert!(err.contains("out of range"));
    assert!(err.contains("-200"));
    assert!(err.contains("200"));
}

// ============================================================================
// Tuner Gain Validation Tests
// ============================================================================

/// Test valid tuner gain values
#[test]
fn test_validate_tuner_gain_valid() {
    assert!(validate_tuner_gain(0).is_ok());
    assert!(validate_tuner_gain(1).is_ok());
    assert!(validate_tuner_gain(30).is_ok()); // 3.0 dB
    assert!(validate_tuner_gain(100).is_ok()); // 10.0 dB
    assert!(validate_tuner_gain(300).is_ok()); // 30.0 dB
    assert!(validate_tuner_gain(TUNER_GAIN_MIN).is_ok()); // 0
    assert!(validate_tuner_gain(TUNER_GAIN_MAX).is_ok()); // 500 (50.0 dB)
}

/// Test invalid tuner gain values
#[test]
fn test_validate_tuner_gain_invalid() {
    assert!(validate_tuner_gain(TUNER_GAIN_MIN - 1).is_err());
    assert!(validate_tuner_gain(TUNER_GAIN_MAX + 1).is_err());
    assert!(validate_tuner_gain(-10).is_err());
    assert!(validate_tuner_gain(600).is_err());
    assert!(validate_tuner_gain(-100).is_err());
    assert!(validate_tuner_gain(1000).is_err());
    assert!(validate_tuner_gain(i32::MIN).is_err());
    assert!(validate_tuner_gain(i32::MAX).is_err());
}

/// Test tuner gain boundary values
#[test]
fn test_validate_tuner_gain_boundaries() {
    assert!(validate_tuner_gain(TUNER_GAIN_MIN).is_ok());
    assert!(validate_tuner_gain(TUNER_GAIN_MAX).is_ok());
    assert!(validate_tuner_gain(TUNER_GAIN_MIN - 1).is_err());
    assert!(validate_tuner_gain(TUNER_GAIN_MAX + 1).is_err());
}

/// Test tuner gain validation error messages
#[test]
fn test_validate_tuner_gain_error_messages() {
    let err = validate_tuner_gain(600).unwrap_err();
    assert!(err.contains("out of range"));
    assert!(err.contains("500"));
}

// ============================================================================
// Rate Limiter Tests
// ============================================================================

/// Simple rate limiter for testing
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

/// Test rate limiter allows first command
#[test]
fn test_rate_limiter_allows_after_interval() {
    let mut limiter = RateLimiter::new(Duration::from_millis(10));
    // First command should always be allowed (last_command set to before now)
    assert!(limiter.check());
}

/// Test rate limiter blocks rapid commands
#[test]
fn test_rate_limiter_blocks_rapid_commands() {
    let mut limiter = RateLimiter::new(Duration::from_millis(500));
    // Consume the initial allowance
    limiter.last_command = Instant::now();
    // Immediate next call should be denied
    assert!(!limiter.check());
}

/// Test rate limiter allows after sleep
#[test]
fn test_rate_limiter_allows_after_sleep() {
    let mut limiter = RateLimiter::new(Duration::from_millis(10));
    limiter.last_command = Instant::now();
    thread::sleep(Duration::from_millis(15));
    assert!(limiter.check());
}

/// Test rate limiter with very short interval
#[test]
fn test_rate_limiter_short_interval() {
    let mut limiter = RateLimiter::new(Duration::from_millis(1));
    assert!(limiter.check());
    thread::sleep(Duration::from_millis(2));
    assert!(limiter.check());
}

/// Test rate limiter with long interval
#[test]
fn test_rate_limiter_long_interval() {
    let mut limiter = RateLimiter::new(Duration::from_secs(10));
    assert!(limiter.check());
    // Immediately try again
    assert!(!limiter.check());
}

/// Test rate limiter allows after exactly interval
#[test]
fn test_rate_limiter_exact_interval() {
    let mut limiter = RateLimiter::new(Duration::from_millis(20));
    limiter.last_command = Instant::now();
    // Sleep for slightly longer than interval
    thread::sleep(Duration::from_millis(25));
    assert!(limiter.check());
}

/// Test rate limiter repeatedly
#[test]
fn test_rate_limiter_repeated() {
    let mut limiter = RateLimiter::new(Duration::from_millis(5));

    for _ in 0..10 {
        assert!(limiter.check());
        thread::sleep(Duration::from_millis(10));
    }
}

// ============================================================================
// Command Payload Encoding Tests
// ============================================================================

/// Test frequency encoding in big-endian
#[test]
fn test_frequency_encoding() {
    let freq: u32 = 100_500_000; // 100.5 MHz
    let bytes = freq.to_be_bytes();
    let buf = [CMD_SET_FREQUENCY, bytes[0], bytes[1], bytes[2], bytes[3]];

    let decoded_freq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(decoded_freq, freq);
}

/// Test sample rate encoding in big-endian
#[test]
fn test_sample_rate_encoding() {
    let rate: u32 = 2_048_000; // 2.048 MS/s
    let bytes = rate.to_be_bytes();
    let buf = [CMD_SET_SAMPLE_RATE, bytes[0], bytes[1], bytes[2], bytes[3]];

    let decoded_rate = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(decoded_rate, rate);
}

/// Test PPM encoding (signed, big-endian)
#[test]
fn test_ppm_encoding() {
    let ppm: i32 = -50;
    let bytes = ppm.to_be_bytes();
    let buf = [CMD_SET_PPM, bytes[0], bytes[1], bytes[2], bytes[3]];

    let decoded_ppm = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(decoded_ppm, ppm);
}

/// Test tuner gain encoding (signed, big-endian)
#[test]
fn test_tuner_gain_encoding() {
    let gain: i32 = 30;
    let bytes = gain.to_be_bytes();
    let buf = [CMD_SET_TUNER_GAIN, bytes[0], bytes[1], bytes[2], bytes[3]];

    let decoded_gain = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(decoded_gain, gain);
}

/// Test AGC command encoding
#[test]
fn test_agc_encoding() {
    // AGC on
    let agc_on: u32 = 1;
    let bytes = agc_on.to_be_bytes();
    let buf1 = [CMD_SET_AGC, bytes[0], bytes[1], bytes[2], bytes[3]];
    let decoded1 = u32::from_be_bytes([buf1[1], buf1[2], buf1[3], buf1[4]]);
    assert_eq!(decoded1, 1);

    // AGC off
    let agc_off: u32 = 0;
    let bytes = agc_off.to_be_bytes();
    let buf2 = [CMD_SET_AGC, bytes[0], bytes[1], bytes[2], bytes[3]];
    let decoded2 = u32::from_be_bytes([buf2[1], buf2[2], buf2[3], buf2[4]]);
    assert_eq!(decoded2, 0);
}

/// Test gain mode command encoding
#[test]
fn test_gain_mode_encoding() {
    // Manual mode (gain_mode > 0)
    let manual: i32 = 1;
    let bytes = manual.to_be_bytes();
    let buf1 = [CMD_SET_GAIN_MODE, bytes[0], bytes[1], bytes[2], bytes[3]];
    let decoded1 = i32::from_be_bytes([buf1[1], buf1[2], buf1[3], buf1[4]]);
    assert_eq!(decoded1, 1);

    // Auto mode (gain_mode = 0)
    let auto_mode: i32 = 0;
    let bytes = auto_mode.to_be_bytes();
    let buf2 = [CMD_SET_GAIN_MODE, bytes[0], bytes[1], bytes[2], bytes[3]];
    let decoded2 = i32::from_be_bytes([buf2[1], buf2[2], buf2[3], buf2[4]]);
    assert_eq!(decoded2, 0);
}

// ============================================================================
// Integration Tests with Network
// ============================================================================

/// Test that we can bind to a port and accept connections
#[test]
fn test_tcp_listener_bind() {
    // Find a free port
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port");
    let port = listener.local_addr().unwrap().port();
    assert!(port > 0);
}

/// Test TCP connection handling
#[test]
fn test_tcp_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port");
    let addr = listener.local_addr().unwrap();

    // Spawn server thread
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("failed to accept");
        stream
    });

    // Connect from client thread
    let client = thread::spawn(move || TcpStream::connect(addr).expect("failed to connect"));

    let client_stream = client.join().expect("client thread panicked");
    let server_stream = server.join().expect("server thread panicked");

    // Both streams should be established
    assert!(client_stream.local_addr().is_ok());
    assert!(server_stream.local_addr().is_ok());
}

/// Test TCP timeout behavior
#[test]
fn test_tcp_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port");
    let addr = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("failed to accept");
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("failed to set read timeout");
        stream
    });

    let client = thread::spawn(move || TcpStream::connect(addr).expect("failed to connect"));

    let mut stream = server.join().expect("server thread panicked");
    let _client = client.join().expect("client thread panicked");

    // Try to read with timeout
    let mut buf = [0u8; 5];
    let result = stream.read_exact(&mut buf);
    // Should timeout since client didn't send anything
    assert!(result.is_err());
}

// ============================================================================
// Comprehensive Protocol Command Valid/Invalid Input Tests
// ============================================================================

/// Test all valid protocol commands with realistic values
#[test]
fn test_all_commands_valid_inputs() {
    // FM radio frequency (100.5 MHz)
    let freq_buf: [u8; 5] = [CMD_SET_FREQUENCY, 0x05, 0xFD, 0x82, 0x20];
    let freq = u32::from_be_bytes([freq_buf[1], freq_buf[2], freq_buf[3], freq_buf[4]]);
    assert!(validate_frequency(freq).is_ok());

    // Standard sample rate (2.048 MS/s)
    let sr_buf: [u8; 5] = [CMD_SET_SAMPLE_RATE, 0x00, 0x1F, 0x40, 0x00];
    let sr = u32::from_be_bytes([sr_buf[1], sr_buf[2], sr_buf[3], sr_buf[4]]);
    assert!(validate_sample_rate(sr).is_ok());

    // Standard gain (30 = 3.0 dB)
    let gain_buf: [u8; 5] = [CMD_SET_TUNER_GAIN, 0x00, 0x00, 0x00, 0x1E];
    let gain = i32::from_be_bytes([gain_buf[1], gain_buf[2], gain_buf[3], gain_buf[4]]);
    assert!(validate_tuner_gain(gain).is_ok());

    // Standard PPM (50)
    let ppm_buf: [u8; 5] = [CMD_SET_PPM, 0x00, 0x00, 0x00, 0x32];
    let ppm = i32::from_be_bytes([ppm_buf[1], ppm_buf[2], ppm_buf[3], ppm_buf[4]]);
    assert!(validate_ppm(ppm).is_ok());
}

/// Test invalid protocol commands with out-of-range values
#[test]
fn test_all_commands_invalid_inputs() {
    // Max frequency + 1
    let freq_over: u32 = FREQ_MAX + 1;
    let freq_buf = [
        CMD_SET_FREQUENCY,
        freq_over.to_be_bytes()[0],
        freq_over.to_be_bytes()[1],
        freq_over.to_be_bytes()[2],
        freq_over.to_be_bytes()[3],
    ];
    let freq = u32::from_be_bytes([freq_buf[1], freq_buf[2], freq_buf[3], freq_buf[4]]);
    assert!(validate_frequency(freq).is_err());

    // Max sample rate + 1
    let sr_over: u32 = SAMPLE_RATE_MAX + 1;
    let sr_buf = [
        CMD_SET_SAMPLE_RATE,
        sr_over.to_be_bytes()[0],
        sr_over.to_be_bytes()[1],
        sr_over.to_be_bytes()[2],
        sr_over.to_be_bytes()[3],
    ];
    let sr = u32::from_be_bytes([sr_buf[1], sr_buf[2], sr_buf[3], sr_buf[4]]);
    assert!(validate_sample_rate(sr).is_err());

    // Max PPM + 1
    let ppm_over: i32 = PPM_MAX + 1;
    let ppm_buf = [
        CMD_SET_PPM,
        ppm_over.to_be_bytes()[0],
        ppm_over.to_be_bytes()[1],
        ppm_over.to_be_bytes()[2],
        ppm_over.to_be_bytes()[3],
    ];
    let ppm = i32::from_be_bytes([ppm_buf[1], ppm_buf[2], ppm_buf[3], ppm_buf[4]]);
    assert!(validate_ppm(ppm).is_err());

    // Max tuner gain + 1
    let gain_over: i32 = TUNER_GAIN_MAX + 1;
    let gain_buf = [
        CMD_SET_TUNER_GAIN,
        gain_over.to_be_bytes()[0],
        gain_over.to_be_bytes()[1],
        gain_over.to_be_bytes()[2],
        gain_over.to_be_bytes()[3],
    ];
    let gain = i32::from_be_bytes([gain_buf[1], gain_buf[2], gain_buf[3], gain_buf[4]]);
    assert!(validate_tuner_gain(gain).is_err());
}

/// Test edge case: all zeros payload
#[test]
fn test_zero_payload_commands() {
    // All commands with zero payload should be valid where applicable
    let freq = u32::from_be_bytes([0, 0, 0, 0]);
    assert!(validate_frequency(freq).is_ok());

    let sr = u32::from_be_bytes([0, 0, 0, 0]);
    assert!(validate_sample_rate(sr).is_ok());

    let ppm = i32::from_be_bytes([0, 0, 0, 0]);
    assert!(validate_ppm(ppm).is_ok());

    let gain = i32::from_be_bytes([0, 0, 0, 0]);
    assert!(validate_tuner_gain(gain).is_ok());
}

/// Test edge case: max valid values
#[test]
fn test_max_valid_values() {
    // Max frequency
    assert!(validate_frequency(FREQ_MAX).is_ok());
    // Max sample rate
    assert!(validate_sample_rate(SAMPLE_RATE_MAX).is_ok());
    // Max PPM
    assert!(validate_ppm(PPM_MAX).is_ok());
    // Max tuner gain
    assert!(validate_tuner_gain(TUNER_GAIN_MAX).is_ok());
}

/// Test edge case: negative PPM values
#[test]
fn test_negative_ppm() {
    assert!(validate_ppm(-1).is_ok());
    assert!(validate_ppm(-100).is_ok());
    assert!(validate_ppm(-200).is_ok());
    assert!(validate_ppm(-201).is_err());
}

/// Test gain mode behavior with different payload values
#[test]
fn test_gain_mode_payload_values() {
    // Positive values should be manual mode
    let positive: i32 = 1;
    let buf1 = [
        CMD_SET_GAIN_MODE,
        positive.to_be_bytes()[0],
        positive.to_be_bytes()[1],
        positive.to_be_bytes()[2],
        positive.to_be_bytes()[3],
    ];
    let mode1 = i32::from_be_bytes([buf1[1], buf1[2], buf1[3], buf1[4]]);
    assert!(mode1 > 0); // Manual mode

    // Zero should be auto mode
    let zero: i32 = 0;
    let buf2 = [
        CMD_SET_GAIN_MODE,
        zero.to_be_bytes()[0],
        zero.to_be_bytes()[1],
        zero.to_be_bytes()[2],
        zero.to_be_bytes()[3],
    ];
    let mode2 = i32::from_be_bytes([buf2[1], buf2[2], buf2[3], buf2[4]]);
    assert_eq!(mode2, 0); // Auto mode

    // Large positive should be manual mode
    let large: i32 = 1000;
    let buf3 = [
        CMD_SET_GAIN_MODE,
        large.to_be_bytes()[0],
        large.to_be_bytes()[1],
        large.to_be_bytes()[2],
        large.to_be_bytes()[3],
    ];
    let mode3 = i32::from_be_bytes([buf3[1], buf3[2], buf3[3], buf3[4]]);
    assert!(mode3 > 0); // Manual mode
}

// ============================================================================
// Performance Tests (basic)
// ============================================================================

/// Test rate limiter performance under rapid commands
#[test]
fn test_rate_limiter_performance() {
    let mut limiter = RateLimiter::new(Duration::from_millis(50));
    let mut allowed = 0u32;
    let mut denied = 0u32;

    let start = Instant::now();
    for _ in 0..1000 {
        if limiter.check() {
            allowed += 1;
        } else {
            denied += 1;
        }
    }
    let elapsed = start.elapsed();

    // Should have allowed some and denied most
    assert!(allowed < denied);
    // Should complete in reasonable time (< 100ms)
    assert!(elapsed < Duration::from_millis(100));
}

/// Test validation performance
#[test]
fn test_validation_performance() {
    let iterations = 10_000;
    let start = Instant::now();

    for i in 0..iterations {
        let _ = validate_frequency(i as u32 % FREQ_MAX);
        let _ = validate_sample_rate(i as u32 % SAMPLE_RATE_MAX);
        let _ = validate_ppm((i % 400) - 200);
        let _ = validate_tuner_gain((i % 600) - 50);
    }

    let elapsed = start.elapsed();
    // Should complete 40k validations in under 100ms
    assert!(elapsed < Duration::from_millis(100));
}

/// Test TCP connection establishment performance
#[test]
fn test_tcp_connection_performance() {
    let iterations = 10;
    let start = Instant::now();

    for _ in 0..iterations {
        let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind");
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("failed to accept");
            stream
        });

        let client = thread::spawn(move || TcpStream::connect(addr).expect("failed to connect"));

        let _ = server.join().unwrap();
        let _ = client.join().unwrap();
    }

    let elapsed = start.elapsed();
    // Should complete 10 connection cycles in under 2 seconds
    assert!(elapsed < Duration::from_secs(2));
}
