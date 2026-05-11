# rtltcp

[![CI](https://github.com/FirebirdRender/rtltcp/workflows/CI/badge.svg)](https://github.com/FirebirdRender/rtltcp/actions)
[![Coverage Status](https://coveralls.io/repos/github/FirebirdRender/rtltcp/badge.svg?branch=main)](https://coveralls.io/github/FirebirdRender/rtltcp?branch=main)
[![Crates.io](https://img.shields.io/crates/v/rtltcp.svg)](https://crates.io/crates/rtltcp)
[![Rust Version](https://img.shields.io/badge/rust-1.74%2B-blue)](https://www.rust-lang.org)

A production-ready Rust implementation of [rtl-tcp](https://github.com/pinkavaj/rtl-sdr/blob/master/src/rtl_tcp.c) with improved stability, better buffering, security hardening, and support for systemd [socket activation](http://0pointer.de/blog/projects/socket-activation.html).

## Features

- **Robust error handling** - No more crashes on client disconnect or device errors
- **Graceful shutdown** - Proper cleanup of device resources and threads
- **Security hardening** - Client logging, DoS protection, input validation
- **Systemd socket activation** - Start on demand, keep USB dongle cool when idle
- **Production tested** - Runs reliably on ARM devices (Odroid, Raspberry Pi)
- **Rate limiting** - Prevents command flooding attacks
- **Input validation** - Protects against out-of-range values reaching hardware

## Security Considerations

### Network Security
- **Default localhost binding**: By default, rtltcp binds to `127.0.0.1` to prevent accidental network exposure
- **DoS protection**: Built-in timeouts (30s default) and rate limiting (50ms command interval) prevent Slowloris and command flooding attacks
- **Input validation**: All protocol commands are validated to prevent out-of-range values from reaching hardware
- **Client logging**: IP addresses are logged on connection for security auditing
- **Warning on all-interfaces binding**: Server warns when binding to 0.0.0.0 or :: to alert administrators

### System Security
- **Systemd hardening**: Service files include security directives to limit process capabilities
- **Minimal privileges**: Process runs with minimal required permissions
- **No root requirements**: Application does not require root privileges for normal operation
- **Memory safety**: Rust's ownership model prevents buffer overflows and use-after-free bugs

## Installation

### Download the latest binary release

Download the [latest release](https://github.com/FirebirdRender/rtltcp/releases) for your platform:

```bash
# x86_64 Linux
wget https://github.com/FirebirdRender/rtltcp/releases/download/v0.6.1/rtltcp-linux-x86_64.tar.gz
tar xzf rtltcp-linux-x86_64.tar.gz
sudo mv rtltcp /usr/local/bin/
chmod +x /usr/local/bin/rtltcp

# ARM64 (aarch64)
# Note: ARM64 binary built on native device due to cross-compilation challenges
# See PROJECT/evaluation-report.md for details
wget https://github.com/FirebirdRender/rtltcp/releases/download/v0.6.1/rtltcp-linux-arm64.tar.gz
tar xzf rtltcp-linux-arm64.tar.gz
sudo mv rtltcp /usr/local/bin/
chmod +x /usr/local/bin/rtltcp
```

### Build from source

Requirements:
- Rust 1.74 or later
- librtlsdr-dev
- libsystemd-dev

```bash
git clone https://github.com/FirebirdRender/rtltcp.git
cd rtltcp
cargo build --release
sudo cp target/release/rtltcp /usr/local/bin/
```

## Usage

### Command Line Options

```
rtltcp 0.6.1
an I/Q spectrum server for RTL2832 based DVB-T receivers

USAGE:
    rtltcp [OPTIONS]

OPTIONS:
    -a, --address <ADDRESS>       listen address [default: 127.0.0.1]
    -p, --port <PORT>             listen port [default: 1234]
    -d, --device-index <INDEX>    device index [default: 0]
    -b, --buffers <COUNT>         number of decoding buffers [default: 15, range: 1-32]
    -s, --tcp-buffers <SIZE>      tcp sending buffer size (bytes) [default: 512000, range: 1-10485760]
    --read-timeout <SECONDS>      socket read timeout [default: 30]
    --write-timeout <SECONDS>     socket write timeout [default: 30]
    -h, --help                    print help
    -V, --version                 print version
```

### Usage Examples for All CLI Flags

#### Basic usage with defaults
Runs on localhost:1234 with device 0:
```bash
rtltcp
```

#### Custom listen address and port
Bind to a specific IP and port:
```bash
rtltcp --address 192.168.1.100 --port 8000
# Or using short flags
rtltcp -a 192.168.1.100 -p 8000
```

#### Bind to all interfaces (use with caution)
```bash
rtltcp --address 0.0.0.0 --port 1234
```

#### Custom device index
Use a specific RTL-SDR device when multiple are connected:
```bash
rtltcp --device-index 1
# Or using short flag
rtltcp -d 1
```

#### Custom buffer settings
Increase buffers for smoother streaming on high-bandwidth connections:
```bash
rtltcp --buffers 20 --tcp-buffers 1024000
# Or using short flags
rtltcp -b 20 -s 1024000
```

#### Custom timeout settings
Increase timeouts for high-latency networks:
```bash
rtltcp --read-timeout 60 --write-timeout 60
```

Reduce timeouts for faster disconnect detection:
```bash
rtltcp --read-timeout 10 --write-timeout 10
```

#### Multiple device setup
Run multiple instances for different devices:
```bash
# Device 0 on port 1234
rtltcp --device-index 0 --port 1234 &
# Device 1 on port 1235
rtltcp --device-index 1 --port 1235 &
```

#### Production-ready with all options
```bash
rtltcp \
  --address 127.0.0.1 \
  --port 1234 \
  --device-index 0 \
  --buffers 20 \
  --tcp-buffers 1024000 \
  --read-timeout 60 \
  --write-timeout 60
```

### Connect with an SDR Client

```bash
# Using gqrx, SDR#, or any rtl-tcp compatible client
# Connect to your server's IP on port 1234
```

### Using Systemd Socket Activation

By using systemd socket activation, rtltcp starts only when a client connects, keeping the RTL-SDR dongle cool when idle.

Create `/etc/systemd/system/rtltcp.service`:

```ini
[Unit]
Description=RTL TCP Service
After=network.target
Requires=rtltcp.socket
ConditionPathExists=/dev/bus/usb/

[Service]
Type=notify
ExecStart=/usr/local/bin/rtltcp
TimeoutStopSec=5

# Security hardening directives
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateUsers=true
ProtectControlGroups=true
ProtectKernelModules=true
ProtectKernelTunables=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
RemoveIPC=true

[Install]
WantedBy=multi-user.target
```

Create `/etc/systemd/system/rtltcp.socket`:

```ini
[Unit]
Description=RTL TCP Socket
PartOf=rtltcp.service

[Socket]
ListenStream=127.0.0.1:1234

[Install]
WantedBy=sockets.target
```

Enable and start:

```bash
sudo systemctl enable rtltcp.socket
sudo systemctl start rtltcp.socket
```

### Hardened Systemd Service File

For production environments, use this hardened configuration with comprehensive security directives:

```ini
[Unit]
Description=RTL TCP Service (Hardened)
After=network.target
Requires=rtltcp.socket
ConditionPathExists=/dev/bus/usb/
ConditionPathExistsGlob=/dev/bus/usb/*/*

[Service]
Type=notify
ExecStart=/usr/local/bin/rtltcp
TimeoutStopSec=5

# User/Group isolation
User=rtlsdr
Group=rtlsdr

# Privilege restrictions
NoNewPrivileges=true
CapabilityBoundingSet=
UMask=0077

# Filesystem protection
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/dev/bus/usb

# Device access
PrivateDevices=false

# Kernel protection
ProtectKernelModules=true
ProtectKernelTunables=true
ProtectControlGroups=true

# Namespace isolation
PrivateUsers=true
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true

# Network restrictions
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX

# Memory protection
MemoryDenyWriteExecute=true

# Cleanup
RemoveIPC=true

# System call filtering
SystemCallFilter=~@privileged @reboot @cpu-emulation @debug @obsolete

[Install]
WantedBy=multi-user.target
```

**Hardening Directives Explained:**

| Directive | Purpose |
|-----------|---------|
| `NoNewPrivileges=true` | Prevents the process from gaining new privileges |
| `ProtectSystem=strict` | Makes the entire file system hierarchy read-only |
| `ProtectHome=true` | Makes /home, /root, /run/user inaccessible |
| `PrivateTmp=true` | Provides an isolated /tmp namespace |
| `PrivateUsers=true` | Runs the service with a private user namespace |
| `ProtectKernelModules=true` | Prevents loading kernel modules |
| `ProtectKernelTunables=true` | Makes /sys and /proc read-only |
| `RestrictAddressFamilies` | Limits socket address families |
| `SystemCallFilter` | Blocks dangerous system call categories |
| `MemoryDenyWriteExecute=true` | Prevents creating writable+executable memory |
| `CapabilityBoundingSet=` | Drops all Linux capabilities |
| `UMask=0077` | Restricts file permissions created by the service |

## Protocol Commands

The rtl-tcp protocol uses a 5-byte command format: `[command_byte][4-byte big-endian payload]`.

| Command | Code | Description | Payload | Valid Range |
|---------|------|-------------|---------|-------------|
| Set Frequency | 0x01 | Set center frequency | u32 (Hz, big-endian) | 0 - 2,200,000,000 |
| Set Sample Rate | 0x02 | Set sample rate | u32 (Hz, big-endian) | 0 - 3,200,000 |
| Set Gain Mode | 0x03 | Set gain mode | i32 (big-endian) | 0 = auto, >0 = manual |
| Set Tuner Gain | 0x04 | Set manual gain | i32 (big-endian, dB*10) | 0 - 500 |
| Set PPM | 0x05 | Set frequency correction | i32 (big-endian, ppm) | -200 - 200 |
| Set AGC | 0x08 | Set automatic gain | u32 (big-endian) | 0 = off, 1 = on |

### Connection Protocol

On connect, the server sends a 12-byte magic packet:
- Bytes 0-3: `"RTL0"` (magic identifier)
- Bytes 4-7: Tuner type (big-endian u32, typically 5 for R820T)
- Bytes 8-11: Maximum gain value (big-endian u32, typically 0x1d)

## Migration Guide

### From v0.3.x to v0.4.0 (and later)

1. **Custom error type**: `RtlTcpError` now replaces `Box<dyn std::error::Error>`. This is transparent for CLI users but affects library consumers.
2. **Enhanced shutdown**: Signal handling is now more responsive with proper stream shutdown.
3. **Improved logging**: Unknown commands are now logged with a running counter.

**No configuration changes required.** Existing command-line invocations work identically.

### From v0.2.x to v0.3.0

1. **BREAKING: Default bind address changed** from `[::]` to `127.0.0.1`
   - **Impact**: Clients connecting from other machines will no longer connect by default
   - **Fix**: Add `--address 0.0.0.0` or `--address [::]` to restore previous behavior
   - **Example**: `rtltcp --address 0.0.0.0 --port 1234`

2. **Input validation added** for all protocol commands
   - **Impact**: Out-of-range values are now rejected silently (logged as warnings)
   - **Fix**: Ensure your client sends values within valid ranges

3. **Rate limiting added** (50ms minimum between commands)
   - **Impact**: Rapid command sequences may be silently dropped
   - **Fix**: Space commands at least 50ms apart if sending programmatically

4. **Read/write timeouts** now default to 30 seconds
   - **Impact**: Idle connections are closed after 30 seconds
   - **Fix**: Increase with `--read-timeout` and `--write-timeout` if needed

### From v0.1.x to v0.2.x

1. **Crash fixes**: Multiple panic-causing bugs fixed. Existing configurations work identically.
2. **Signal handling**: Improved SIGINT/SIGTERM handling for clean shutdown.
3. **Command 0x03 fix**: Manual gain mode now correctly disables AGC (previously was backwards).

## Building

```bash
# Development build
cargo build

# Release build (with optimizations)
cargo build --release

# Run tests
cargo test --all-features

# Run with debug logging
RUST_LOG=debug cargo run -- --port 1234

# Run with info logging
RUST_LOG=info cargo run -- --port 1234
```

## System Requirements

- RTL2832-based DVB-T receiver (RTL-SDR)
- Linux with libusb 1.0+
- Rust 1.74+ (for building from source)

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.