# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-05-10

### Added
- Comprehensive test coverage with 150+ test cases across all modules
- Enhanced documentation with security considerations and migration guide
- Mock device abstraction for testing
- Performance testing framework
- Edge case testing for all protocol commands
- Systemd service hardening example

### Changed
- Replaced `Box<dyn std::error::Error>` with custom `RtlTcpError` type throughout the application
- Improved unknown command logging with warning level and counter
- Enhanced test coverage and documentation

## [0.4.0] - 2026-05-10

### Added
- Custom error type (`RtlTcpError`) with proper error handling throughout the application
- Enhanced shutdown handling for responsive signal processing

### Changed
- Replaced `Box<dyn std::error::Error>` with custom `RtlTcpError` type throughout the application
- Improved unknown command logging with warning level and counter

## [0.3.0] - 2026-05-10

## [0.3.0] - 2026-05-10

### Added
- Configurable read/write timeout values via `--read-timeout` and `--write-timeout` CLI flags
- Rate limiting for commands to prevent command flooding (50ms minimum interval)
- Input validation for protocol payloads (frequency, sample rate, gain ranges, PPM values)
- Warning when binding to all interfaces for security awareness

### Changed
- **BREAKING**: Default bind address changed from `[::]` to `127.0.0.1` for security
- Enhanced command processing with input validation and bounds checking

### Security
- Default localhost binding prevents accidental network exposure
- Rate limiting prevents command flooding attacks
- Input validation prevents out-of-range values from reaching device

## [0.2.1] - 2026-05-10

### Added
- Client IP address logging on connection for security auditing
- Read/write timeout protection against Slowloris DoS attacks
- Input validation with bounds checking for CLI arguments
- Comprehensive unit tests for all protocol command parsing
- Protocol parsing tests for unknown commands

### Changed
- Replaced duplicated mutex handling with `with_control` helper function
- Enhanced error logging to include actual error messages instead of discarding them
- Improved error context on device and system calls with proper error chaining
- Fixed cross-compilation workflow to properly build all target platforms
- Added MSRV verification job to CI pipeline
- Enhanced streaming failure logging with error details

### Fixed
- Critical crash issues that caused panics on client disconnects
- Command 0x03 logic error where manual gain incorrectly enabled AGC
- Mutex poisoning and channel deadlock problems
- Error values silently discarded in device operations

### Security
- Added client IP address logging for audit trail
- Added 30-second read/write timeouts to prevent DoS attacks
- Enhanced input validation with reasonable bounds for all CLI arguments

## [0.2.0] - 2026-05-10

### Added
- Comprehensive error handling throughout the application to prevent crashes
- Unit tests for protocol parsing and magic packet validation
- Integration tests for binary functionality
- Named constants for RTL-TCP protocol command codes
- Release profile optimizations (LTO, codegen-units, strip)

### Changed
- Fixed critical stability issues that caused panics on client disconnects
- Replaced all unsafe `.unwrap()` calls with proper error handling
- Fixed command 0x03 logic bug (manual gain was incorrectly enabling AGC)
- Improved shutdown handling to ensure clean resource cleanup
- Fixed compile error in non-systemd code path
- Fixed channel deadlock that caused immediate process exits
- Fixed normal-completion hang when read_async completes normally
- Fixed mutex poisoning cascade that caused multi-panic crashes
- Updated CI/CD workflows with modern GitHub Actions
- Updated dependencies and MSRV to Rust 1.74

### Security
- Removed abandoned GitHub Actions dependencies

## [0.1.1] - 2024-01-15

### Added
- Initial release with basic rtl-tcp functionality
- Support for systemd socket activation
- Better buffering compared to original rtl-tcp implementation