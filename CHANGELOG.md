# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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