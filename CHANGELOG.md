# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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