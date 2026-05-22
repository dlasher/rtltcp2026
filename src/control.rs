use std::net::IpAddr;
use std::ops::RangeInclusive;
use std::result::Result as StdResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ipnet::IpNet;
use tracing::warn;

use crate::error::RtlTcpError;

/// RTL-TCP protocol command codes
pub const COMMAND_HEADER_SIZE: usize = 5;
pub const CMD_SET_FREQUENCY: u8 = 0x01;
pub const CMD_SET_SAMPLE_RATE: u8 = 0x02;
pub const CMD_SET_GAIN_MODE: u8 = 0x03;
pub const CMD_SET_TUNER_GAIN: u8 = 0x04;
pub const CMD_SET_PPM: u8 = 0x05;
pub const CMD_SET_AGC: u8 = 0x08;
pub const CMD_CHAIN_DETECT: u8 = 0xF0;

/// Magic packet sent to client on connect:
/// "RTL0" (4 bytes) + tuner type 5 (4 bytes BE) + max gain value 0x1d (4 bytes BE)
pub const MAGIC_PACKET: &[u8] = b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d";

/// Valid frequency range for RTL-SDR devices (0 Hz to 2.2 GHz)
pub const FREQ_MIN: u32 = 0;
pub const FREQ_MAX: u32 = 2_200_000_000;
pub const FREQ_RANGE: RangeInclusive<u32> = FREQ_MIN..=FREQ_MAX;

/// Valid sample rate range (0 Hz to 3.2 MHz)
pub const SAMPLE_RATE_MIN: u32 = 0;
pub const SAMPLE_RATE_MAX: u32 = 3_200_000;
pub const SAMPLE_RATE_RANGE: RangeInclusive<u32> = SAMPLE_RATE_MIN..=SAMPLE_RATE_MAX;

/// Valid PPM correction range (-200 to 200)
pub const PPM_MIN: i32 = -200;
pub const PPM_MAX: i32 = 200;
pub const PPM_RANGE: RangeInclusive<i32> = PPM_MIN..=PPM_MAX;

/// Valid tuner gain range (0 to 500, representing 0 to 50 dB in 0.1 dB steps)
pub const TUNER_GAIN_MIN: i32 = 0;
pub const TUNER_GAIN_MAX: i32 = 500;
pub const TUNER_GAIN_RANGE: RangeInclusive<i32> = TUNER_GAIN_MIN..=TUNER_GAIN_MAX;

/// Minimum interval between commands to prevent flooding (50 ms)
pub const COMMAND_RATE_LIMIT_INTERVAL: Duration = Duration::from_millis(50);

/// Execute an operation on the device control handle, handling mutex poisoning gracefully.
pub fn with_control<T, F>(ctl: &std::sync::Mutex<T>, op: F)
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
pub fn validate_frequency(freq: u32) -> Result<(), String> {
    if !FREQ_RANGE.contains(&freq) {
        Err(format!(
            "frequency {freq} Hz out of range ({FREQ_MIN}-{FREQ_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Validate a sample rate value, returning an error message if out of bounds.
pub fn validate_sample_rate(rate: u32) -> Result<(), String> {
    if !SAMPLE_RATE_RANGE.contains(&rate) {
        Err(format!(
            "sample rate {rate} Hz out of range ({SAMPLE_RATE_MIN}-{SAMPLE_RATE_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Validate a PPM correction value, returning an error message if out of bounds.
pub fn validate_ppm(ppm: i32) -> Result<(), String> {
    if !PPM_RANGE.contains(&ppm) {
        Err(format!("ppm {ppm} out of range ({PPM_MIN}-{PPM_MAX})"))
    } else {
        Ok(())
    }
}

/// Validate a tuner gain value, returning an error message if out of bounds.
pub fn validate_tuner_gain(gain: i32) -> Result<(), String> {
    if !TUNER_GAIN_RANGE.contains(&gain) {
        Err(format!(
            "tuner gain {gain} out of range ({TUNER_GAIN_MIN}-{TUNER_GAIN_MAX})"
        ))
    } else {
        Ok(())
    }
}

/// Check if an IP address passes the whitelist, returning Ok if allowed, Err if rejected.
pub fn check_whitelist(client_ip: &str, whitelist: &[String]) -> StdResult<(), RtlTcpError> {
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
pub fn is_ip_in_whitelist(client_ip: &str, whitelist: &[String]) -> bool {
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
pub struct AgcState {
    enabled: AtomicBool,
}

impl AgcState {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
        }
    }

    pub fn enable(&self) -> bool {
        !self.enabled.swap(true, Ordering::SeqCst)
    }

    pub fn disable(&self) -> bool {
        self.enabled.swap(false, Ordering::SeqCst)
    }
}

/// Simple rate limiter that tracks the time of the last allowed command.
pub struct RateLimiter {
    last_command: Instant,
    min_interval: Duration,
}

impl RateLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_command: Instant::now()
                .checked_sub(min_interval)
                .unwrap_or(Instant::now()),
            min_interval,
        }
    }

    /// Check if a command is allowed under the rate limit.
    /// Returns true if allowed, false if the command should be rejected.
    pub fn check(&mut self) -> bool {
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
