# rtltcp-2026

[![CI](https://github.com/FirebirdRender/rtltcp2026/workflows/CI/badge.svg)](https://github.com/FirebirdRender/rtltcp2026/actions)
[![Coverage Status](https://coveralls.io/repos/github/FirebirdRender/rtltcp2026/badge.svg?branch=main)](https://coveralls.io/github/FirebirdRender/rtltcp2026?branch=main)
[![Crates.io](https://img.shields.io/crates/v/rtltcp.svg)](https://crates.io/crates/rtltcp)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)

Fork of [niclashoyer/rtltcp](https://github.com/niclashoyer/rtltcp) with stability, security, and performance improvements for production use.

## Key Enhancements over Original

### Security Hardening
- Server enforces 30s read/write timeouts and 50ms command rate limiting to prevent connection exhaustion and flooding attacks.
- Protocol commands are bounds-checked before reaching hardware.
- Default bind address is `127.0.0.1`, not all interfaces.
- systemd service files ship with namespace isolation, capability dropping, and syscall filtering.

### Stability & Performance
- TCP buffer management flushes each USB transfer batch immediately. Clients like rtl_433 no longer stall waiting for data.
- `RtlTcpError` replaces boxed errors. Client disconnects don't panic the server.
- Signal handling cleans up device resources and threads on shutdown.

### Tooling & Quality
- 150+ test cases cover edge cases and protocol parsing.
- Dependencies use semver-compatible ranges with `Cargo.lock` for reproducible builds.

## Features
- Custom error type prevents crashes on client disconnect or device errors.
- Signal handler shuts down device resources and threads cleanly.
- Client IP logging, DoS protection, input validation.
- systemd socket activation keeps the USB dongle cool when idle.
- Runs reliably on Linux (x86_64, ARM via local build).
- Rate limiting prevents command flooding attacks.
- Input validation rejects out-of-range values before they reach hardware.

## Security Considerations

### Network Security
- Listens on `127.0.0.1` by default. No accidental network exposure.
- 30s timeouts and 50ms command interval prevent Slowloris and command flooding.
- All command payloads validated against hardware-safe ranges.
- Client IP logged on connection for security auditing.
- Server warns when binding to `0.0.0.0` or `::`.

### System Security
- systemd service files restrict process capabilities, namespaces, and syscalls.
- Process runs with minimal required permissions.
- No root privileges required for normal operation.
- Rust's ownership model prevents buffer overflows and use-after-free bugs.

## Installation

### Download the latest binary release

Grab the [latest release](https://github.com/FirebirdRender/rtltcp2026/releases):

```bash
# x86_64 Linux
wget https://github.com/FirebirdRender/rtltcp2026/releases/download/v0.7.4/rtltcp-v0.7.4-linux-x86_64.tar.gz
tar xzf rtltcp-v0.7.4-linux-x86_64.tar.gz
sudo mv rtltcp /usr/local/bin/
chmod +x /usr/local/bin/rtltcp
```

### Build from source

Requirements:
- Rust 1.75 or later
- librtlsdr-dev
- libsystemd-dev

```bash
git clone https://github.com/FirebirdRender/rtltcp2026.git
cd rtltcp2026
cargo build --release
sudo cp target/release/rtltcp /usr/local/bin/
```

### Building for ARM (aarch64)

Pre-built binaries only cover x86_64 Linux. On ARM hardware (Odroid, Raspberry Pi):

```bash
sudo apt install -y librtlsdr-dev libsystemd-dev build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
git clone https://github.com/FirebirdRender/rtltcp2026.git
cd rtltcp2026
cargo build --release
sudo cp target/release/rtltcp /usr/local/bin/
```

Builds natively on the target device. No cross-compilation toolchain needed.

## Usage

### Command Line Options

```
rtltcp 0.7.4
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
```bash
rtltcp --address 192.168.1.100 --port 8000
rtltcp -a 192.168.1.100 -p 8000
```

#### Bind to all interfaces (use with caution)
```bash
rtltcp --address 0.0.0.0 --port 1234
```

#### Custom device index
```bash
rtltcp --device-index 1
rtltcp -d 1
```

#### Custom buffer settings
Increase USB transfer buffers for smoother streaming on high-bandwidth connections:
```bash
rtltcp --buffers 20
rtltcp -b 20
```

`--tcp-buffers` / `-s` controls the userspace buffer size for outgoing TCP writes. Each USB transfer buffer flushes immediately. Values above 512KB can cause clients like rtl_433 to time out waiting for data. Only increase this for clients that buffer incoming data.

#### Custom timeout settings
```bash
rtltcp --read-timeout 60 --write-timeout 60
rtltcp --read-timeout 10 --write-timeout 10
```

#### Multiple device setup
```bash
rtltcp --device-index 0 --port 1234 &
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

systemd socket activation starts rtltcp only when a client connects. The RTL-SDR dongle stays cool when idle.

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

For production environments:

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

| Directive | Purpose |
|-----------|---------|
| `NoNewPrivileges=true` | Prevents the process from gaining new privileges |
| `ProtectSystem=strict` | Makes file system hierarchy read-only |
| `ProtectHome=true` | Blocks access to /home, /root, /run/user |
| `PrivateTmp=true` | Isolates /tmp namespace |
| `PrivateUsers=true` | Runs with private user namespace |
| `ProtectKernelModules=true` | Prevents loading kernel modules |
| `ProtectKernelTunables=true` | Makes /sys and /proc read-only |
| `RestrictAddressFamilies` | Limits socket address families |
| `SystemCallFilter` | Blocks dangerous system call categories |
| `MemoryDenyWriteExecute=true` | Prevents writable+executable memory |
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

1. `RtlTcpError` replaces `Box<dyn std::error::Error>`. Transparent for CLI users but affects library consumers.
2. Signal handling shuts down the stream on SIGINT/SIGTERM.
3. Unknown commands log with a running counter.

Command-line invocations from v0.3.x work without changes.

### From v0.2.x to v0.3.0

1. Default bind address changed from `[::]` to `127.0.0.1`. Add `--address 0.0.0.0` or `--address [::]` for network access.
2. Protocol commands are validated. Out-of-range values are rejected silently.
3. Rate limiting allows 1 command per 50ms minimum. Rapid sequences may be silently dropped.
4. Read/write timeouts default to 30 seconds. Increase with `--read-timeout` and `--write-timeout` if needed.

### From v0.1.x to v0.2.x

1. Multiple panic-causing bugs fixed. Existing configurations work identically.
2. Improved SIGINT/SIGTERM handling for clean shutdown.
3. Command 0x03 now correctly disables AGC when setting manual gain mode (previously backwards).

## Building

```bash
cargo build
cargo build --release
cargo test --all-features
RUST_LOG=debug cargo run -- --port 1234
RUST_LOG=info cargo run -- --port 1234
```

## System Requirements

- RTL2832-based DVB-T receiver (RTL-SDR)
- Linux with libusb 1.0+
- Rust 1.75+ (for building from source)

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
