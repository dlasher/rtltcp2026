# Changelog
## [0.10.1] - 2026-05-24

### Fixed
- **CI workflows**: Replaced `--all-features` with `--features daemon_systemd` across all CI jobs to exclude hardware-dependent tests. The `hardware-tests` feature requires physical RTL-SDR devices and was causing spurious failures in CI (coverage, test, clippy, docs).
- **MSRV clippy**: Fixed `needless_borrows_for_generic_args` at `src/main.rs:158` (`TcpListener::bind(&addr)` → `TcpListener::bind(addr)`).
- **Code formatting**: Ran `cargo fmt` to satisfy the Rustfmt CI check.

## [0.10.0] - 2026-05-24

### Added
- **`rtltcp-capture` binary**: Connects to an RTL_TCP server and saves IQ data with timestamps to a self-describing file format (`RTLX` magic). Supports configurable duration, timeout, and in-memory buffer threshold (`--buffer-mem`). Captures Ctrl-C gracefully with final flush and stats.
- **`rtltcp-replay` binary**: Replays captured IQ data as a minimal RTL_TCP server. Supports speed control (`--speed 0` = async, `1.0` = realtime), loop mode (`--loop`), and logs all client commands (frequency, sample rate, gain, AGC, etc.) with decoded values.
- **Capture file format**: Chunked binary format with `RTLX` magic (4 bytes), version (u32 LE), magic payload length + payload, followed by timestamped chunks (timestamp u64 LE, length u32 LE, data).
- **Command logging test** (`TASK-016`): Integration test verifying all three command types (SET_FREQUENCY, SET_SAMPLE_RATE, CHAIN_DETECT) appear in replay stderr output.
- **Error-path tests** (`TASK-018`): 6 automated tests covering missing file, corrupted magic, empty file, connection failure, invalid arguments, and clean disconnect.

### Fixed
- **Replay command reader thread**: Changed from polling `cmd_quit` before each `read_exact` to using `set_read_timeout(200ms)` with `checked`-quit-on-timeout. Previously, buffered commands could be discarded if the main thread finished and set the quit flag while commands were pending in the TCP receive buffer.
- **`--loop-mode` renamed to `--loop`**: CLI flag now matches the design spec (REQ-012/TASK-010). The `--loop-mode` flag still works as long-form fallback.
- **Replay tracing output**: Explicitly configured to go to stderr (previously defaulted to stdout), consistent with tracing-subscriber conventions.

## [0.9.4] - 2026-05-22

### Fixed
- **Cipher lock poison recovery**: Proxy control thread now logs a warning and recovers from a poisoned `write_cipher` Mutex instead of silently forwarding commands in plaintext.

### Changed
- **Strengthened encrypted command test**: Expanded to verify keystream continuation across multiple consecutive commands, replacing the previous single-command assertion. Replaced fragile `sleep(50ms)` with channel-based synchronization.

## [0.9.3] - 2026-05-22

### Fixed
- **Proxy command encryption**: Proxy control thread now encrypts forwarded commands with ChaCha20 when upstream connection is encrypted. Previously commands were sent in plaintext while server expected encrypted data, producing "unsupported command" warnings on the upstream server.
- **Build warnings eliminated**: removed unused `warn` import, suppressed dead-code warnings on test-only `EncryptedReader`/`EncryptedReader::new`, prefixed unused `_write_cipher`/`_receiver`/`_receiver`.

### Changed
- `UpstreamConnection.encryption_key` replaced with `read_cipher`/`write_cipher` (ChaCha20 ciphers). `connect_upstream` creates ciphers from exchanged nonces. Downstream consumers use `write_cipher` directly instead of reconstructing ciphers from raw nonces.

## [0.9.2] - 2026-05-22

### Fixed
- **Ctrl-C now cleanly shuts down the server**: Master listener set to non-blocking mode with a poll loop. Previously `accept()` blocked indefinitely, preventing the shutdown signal from being checked.
- **Non-blocking shutdown in proxy mode**: Same fix applied to `run_proxy_multi`.

## [0.9.1] - 2026-05-22

### Fixed
- **Master now receives IQ data**: Subscribed master to broadcast channel in multi-client serve mode. Previously master got magic packet but no IQ data — only slaves were wired.
- **Control thread no longer dies on idle timeout**: `WouldBlock` and `TimedOut` now `continue` instead of `break`. Linux `SO_RCVTIMEO` fires as `EAGAIN`/`WouldBlock` after `read-timeout` seconds; previously killed the control thread, making the server unrecoverable.
- **Master reconnection**: Both serve and proxy modes now loop back to `accept()` after master disconnect. `read_async` runs on its own thread so it doesn't block re-accept. Previously a single accept+control thread meant master reconnect was impossible without restart.
- **Proxy upstream stream thread safety**: Wrapped upstream control stream in `Arc<Mutex<>>` to survive master reconnection loop.
- **Master-port/slave-port conflict**: `validate_args` rejects identical ports with a clear error instead of cryptic `AddrInUse`.

### Added
- Integration test: `test_master_slave_same_port_rejected` — verifies port conflict validation.

## [0.9.0] - 2026-05-22

### Added
- **Multi-client serve mode**: One USB device drives a master connection, up to 10 slave connections receive identical IQ data read-only via `tokio::sync::broadcast` fan-out
- **Proxy mode**: Chain rtltcp servers together with `--mode proxy --upstream host:port` to relay IQ across the network
- **Chain detection**: Downstream proxies probe upstream with reserved opcode `0xF0` (500ms timeout) to detect proxy protocol support
- **ChaCha20 encryption**: Optional encryption between chained proxies via `--key <hex>` or `--key-file <path>` with automatic nonce exchange
- **New CLI flags**: `--mode`, `--slave-port`, `--max-slaves`, `--upstream`, `--key`, `--key-file`
- **New dependencies**: `tokio` (sync), `chacha20`, `rand`, `hex`
- `hardware-tests` Cargo feature for real-device integration tests
- 170+ test cases covering broadcast fan-out, chain detection, encryption round-trips, protocol commands, and graceful shutdown

### Changed
- `--port` aliased to `--master-port`. The `-p` short flag still works. Existing invocations unchanged
- Control module extracted to `src/control.rs` (constants, validation, rate limiter, whitelist, AGC state)
- Stream module created in `src/stream.rs` (broadcast channel + per-client writer loop)
- Proxy module created in `src/proxy.rs` (upstream connection, chain detection)
- Encryption module created in `src/encryption.rs` (EncryptedReader/Writer, nonce exchange)
- Single-client mode preserved as `run_serve_single` with identical behavior
- README fully updated with new CLI, usage examples, protocol docs, and migration guide

## [0.7.4] - 2026-05-14

### Fixed
- Restored BufWriter with per-buffer flush to fix rtl_433 stalls (while keeping the tcp_buffers CLI flag)
- Added bytes-sent diagnostic for read_async troubleshooting


All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.3] - 2026-05-13

### Fixed
- **Spurious gain mode logging**: Only log gain mode changes when the state actually changes, eliminating repeated log spam from clients that repeatedly send the same gain mode command

## [0.7.2] - 2026-05-13

### Fixed
- **IP whitelist IPv4-mapped IPv6 addresses**: Normalize IPv4-mapped IPv6 addresses (::ffff:a.b.c.d) to IPv4 before CIDR matching, fixing whitelist rejection when binding to 0.0.0.0

### Changed
- Simplified CI/CD to x86_64 native builds only, removed ARM cross-compilation attempts
- Removed cargo.io publishing from CD workflow

### CI/CD
- Fixed rustfmt issues across all source files
- Fixed MSRV job missing clippy component
- Fixed publish dry-run failing on Cargo.lock dirty state

## [0.7.1] - 2026-05-11

### Changed
- Removed "Publishing to Cargo" job from CD workflow

## [0.7.1] - 2026-05-11

### Changed
- Simplified CI/CD to x86_64 native builds only, removed ARM cross-compilation attempts
- Removed cargo.io publishing from CD workflow

## [0.7.0] - 2026-05-11

### Changed
- Simplified CD workflow to x86_64 native builds only
- Added permissions: contents: write to CD release job
- Fixed release step to upload files directly with create command

### CI/CD
- Fixed rustfmt issues across all source files (48+ diffs)
- Added clippy component to MSRV test job
- Added --allow-dirty to publish dry-run
- Regenerated Cargo.lock for MSRV compatibility (v3 format)

## [0.6.1] - 2026-05-11

### Fixed
- **Clippy lint fixes**: Renamed error variants from `*Error` to without suffix to fix `clippy::enum_variant_names`
- **Clippy lint fixes**: Replaced manual range checks with `RangeInclusive::contains()` method and defined range constants
- **Clippy lint fixes**: Replaced `unwrap_or_else` with `unwrap_or` in RateLimiter for performance improvement
- **Clippy lint fixes**: Removed unnecessary `i32 as i32` casts and renamed unused variables in tests

### Security
- **Dependency pinning**: All dependencies pinned to exact versions (`=X.Y.Z`) for reproducible builds
- **Security gate**: Added `fail_on_advisories: true` to audit.yml workflow to fail on security vulnerabilities

## [0.6.0] - 2026-05-10

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
- Simplified CD workflow to x86_64 native builds only
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