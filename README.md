# rtltcp-2026

[![CI](https://github.com/dlasher/rtltcp2026/workflows/CI/badge.svg)](https://github.com/dlasher/rtltcp2026/actions)
[![Crates.io](https://img.shields.io/crates/v/rtltcp2026.svg)](https://crates.io/crates/rtltcp2026)
[![Rust Version](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)

Fork of [niclashoyer/rtltcp](https://github.com/niclashoyer/rtltcp) with stability, security, and performance improvements for production use.

## Key Enhancements over Original

### Multi-Client Proxy Architecture (v0.9.0+)
- **Serve mode** with a driver (master) connection and up to 10 read-only consumer (slave) connections. One USB device, many clients.
- **Proxy mode** chains multiple rtltcp servers together, relaying IQ data across the network. Upstream chain detection uses a 0xF0 protocol probe.
- **Optional ChaCha20 encryption** between chained proxies secures IQ data in transit.
- **tokio::sync::broadcast** fan-out delivers identical IQ data to every connected client from a single USB read loop.

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
- 170+ test cases cover edge cases, protocol parsing, broadcast fan-out, chain detection, encryption round-trips, and graceful shutdown.
- Dependencies use semver-compatible ranges with `Cargo.lock` for reproducible builds.

## Features
- **Multi-client serve**: One USB device, one master client drives commands, up to 10 slave clients receive IQ data.
- **Proxy chaining**: Chain rtltcp servers together. Supports optional ChaCha20 encryption between links.
- **Chain detection**: Downstream proxies automatically detect upstream proxy support via 0xF0 protocol probe with 500ms timeout.
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
- **Multi-client**: Master connection drives hardware commands; slave connections are write-only (no hardware access).
- **Proxy**: Upstream connection can optionally use ChaCha20 encryption with 32-byte key exchange.

### System Security
- systemd service files restrict process capabilities, namespaces, and syscalls.
- Process runs with minimal required permissions.
- No root privileges required for normal operation.
- Rust's ownership model prevents buffer overflows and use-after-free bugs.

## Installation

### Download the latest binary release

Grab the [latest release](https://github.com/dlasher/rtltcp2026/releases):

```bash
# x86_64 Linux
wget https://github.com/dlasher/rtltcp2026/releases/download/v0.8.0/rtltcp2026-v0.8.0-linux-x86_64.tar.gz
tar xzf rtltcp2026-v0.8.0-linux-x86_64.tar.gz
sudo mv rtltcp2026 /usr/local/bin/
chmod +x /usr/local/bin/rtltcp2026

# aarch64 Linux (Odroid, Raspberry Pi 4/5, etc.)
wget https://github.com/dlasher/rtltcp2026/releases/download/v0.8.0/rtltcp2026-v0.8.0-linux-aarch64.tar.gz
tar xzf rtltcp2026-v0.8.0-linux-aarch64.tar.gz
sudo mv rtltcp2026 /usr/local/bin/
chmod +x /usr/local/bin/rtltcp2026
```

### Build from source

Requirements:
- Rust 1.75 or later
- librtlsdr-dev
- libsystemd-dev

```bash
git clone https://github.com/dlasher/rtltcp2026.git
cd rtltcp2026
cargo build --release
sudo cp target/release/rtltcp2026 /usr/local/bin/
```

### Building for ARM (aarch64)

Pre-built binaries only cover x86_64 Linux. On ARM hardware (Odroid, Raspberry Pi):

```bash
sudo apt install -y librtlsdr-dev libsystemd-dev build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
git clone https://github.com/dlasher/rtltcp2026.git
cd rtltcp2026
cargo build --release
sudo cp target/release/rtltcp2026 /usr/local/bin/
```

Builds natively on the target device. No cross-compilation toolchain needed.

## Usage

### Command Line Options

```
rtltcp2026

an I/Q spectrum server for RTL2832 based DVB-T receivers

Usage: rtltcp2026 [OPTIONS]

Options:
      --mode <MODE>                    operating mode: "serve" (default) or "proxy"
                                       [default: serve] [possible values: serve, proxy]
  -a, --address <ADDRESS>              listen address [default: 127.0.0.1]
  -p, --master-port <MASTER_PORT>      master port — accepts the driver connection
                                       (alias for --port) [default: 1234]
      --slave-port <SLAVE_PORT>        slave port — accepts read-only consumer connections
      --max-slaves <MAX_SLAVES>        maximum number of connected slaves [default: 10]
      --upstream <UPSTREAM>            upstream rtltcp server (host:port) for proxy mode
      --key <KEY>                      hex-encoded 32-byte encryption key
      --key-file <KEY_FILE>            path to 32-byte raw encryption key file
  -d, --device-index <DEVICE_INDEX>    device index [default: 0]
  -b, --buffers <BUFFERS>              number of decoding buffers [default: 15]
  -s, --tcp-buffers <TCP_BUFFERS>      tcp sending buffer size (bytes)
                                       [default: 512000, range: 1-10485760]
      --read-timeout <READ_TIMEOUT>    socket read timeout in seconds [default: 30]
      --write-timeout <WRITE_TIMEOUT>  socket write timeout in seconds [default: 30]
      --whitelist <WHITELIST>          IP whitelist (CIDR notation)
  -h, --help                           Print help
  -V, --version                        Print version
```

### Modes

**serve** (default) — Attach to a local RTL-SDR device. Accept one master client that drives commands and up to 10 slave clients that receive IQ data read-only.

**proxy** — Connect to an upstream rtltcp server and relay IQ data to local clients. Optionally encrypt the upstream link with ChaCha20.

### Usage Examples

#### Basic usage (single client, backward compatible)

```bash
rtltcp2026                          # localhost:1234, device 0
rtltcp2026 -p 8000                  # custom port
rtltcp2026 --address 0.0.0.0        # all interfaces
```

#### Multi-client serve: one USB, many consumers

Start the server with a slave port:

```bash
rtltcp2026 \
  --mode serve \
  --master-port 1234 \
  --slave-port 1235
```

Connect the master (drives hardware — frequency, gain, etc.):

```bash
# gqrx, rtl_433, or custom client on port 1234
```

Connect one or more read-only slaves:

```bash
# Additional clients on port 1235 — all receive identical IQ data
# rtl_433 --direct ... or another rtltcp-compatible client
```

Master commands set frequency and gain; slaves just consume IQ data. Slaves cannot send commands to the hardware.

#### Proxy chain

Connect two rtltcp servers, where the downstream serves multiple clients from upstream IQ:

```bash
# Upstream: local SDR on port 9991
rtltcp2026 --mode serve --master-port 9991

# Downstream proxy: relays upstream IQ to local clients
rtltcp2026 \
  --mode proxy \
  --master-port 1234 \
  --slave-port 1235 \
  --upstream 127.0.0.1:9991
```

Connect the downstream master to proxy (relays commands upstream):

```bash
# This client's frequency/gain/etc commands are forwarded to the upstream SDR
rtltcp2026 --port 1234   # connects to the proxy as master
# Or any rtl-tcp compatible client
```

Connect read-only slaves to the proxy:

```bash
# These clients receive IQ data relayed from upstream via the proxy
# Multiple slaves on port 1235 get identical IQ data
```

#### Encrypted proxy chain

Secure IQ data between proxies with ChaCha20:

```bash
# Generate a 32-byte key
head -c 32 /dev/urandom > /etc/rtltcp/proxy.key

# Upstream SDR server
rtltcp2026 --mode serve --master-port 9991

# Downstream proxy with encryption
rtltcp2026 \
  --mode proxy \
  --master-port 1234 \
  --slave-port 1235 \
  --upstream 127.0.0.1:9991 \
  --key-file /etc/rtltcp/proxy.key
```

Or provide the key as a hex string: `--key <64-hex-chars>`.

#### Maximum slaves

```bash
rtltcp2026 \
  --mode serve \
  --master-port 1234 \
  --slave-port 1235 \
  --max-slaves 5
```

Limits concurrent slave connections. Default is 10.

#### Custom timeout settings

```bash
rtltcp2026 --read-timeout 60 --write-timeout 60
rtltcp2026 --read-timeout 10 --write-timeout 10
```

#### Production-ready multi-client

```bash
rtltcp2026 \
  --address 127.0.0.1 \
  --master-port 1234 \
  --slave-port 1235 \
  --max-slaves 10 \
  --device-index 0 \
  --buffers 20 \
  --tcp-buffers 1024000 \
  --read-timeout 60 \
  --write-timeout 60
```

#### Multiple device setup (legacy single-client)

```bash
rtltcp2026 --device-index 0 --port 1234 &
rtltcp2026 --device-index 1 --port 1235 &
```

### Connect with an SDR Client

```bash
# Using gqrx, SDR#, rtl_433, or any rtl-tcp compatible client
# Connect to your server's IP on port 1234 (master) or port 1235 (slave)
```

### Using Systemd Socket Activation

systemd socket activation starts rtltcp only when a client connects. The RTL-SDR dongle stays cool when idle.

Only the **master port** supports socket activation. Slave ports are auto-opened by the process.

Create `/etc/systemd/system/rtltcp.service`:

```ini
[Unit]
Description=RTL TCP Service
After=network.target
Requires=rtltcp.socket
ConditionPathExists=/dev/bus/usb/

[Service]
Type=notify
ExecStart=/usr/local/bin/rtltcp2026 --mode serve --slave-port 1235
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
ExecStart=/usr/local/bin/rtltcp2026 --mode serve --slave-port 1235
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
| Chain Detect | 0xF0 | Probe for proxy chain support | magic "PROX" | (reserved) |

The **Chain Detect** command (0xF0) is a reserved opcode not defined in the original rtl-tcp spec. It is used internally by proxy-mode servers to detect chain support: a downstream proxy sends `[0xF0, 0x50, 0x52, 0x4F, 0x58]` and expects `[0xF0, 0x00, 0x00, 0x00, 0x00]` back within 500ms. If no ACK arrives, the connection proceeds in plain TCP mode.

### Connection Protocol

On connect, the server sends a 12-byte magic packet:
- Bytes 0-3: `"RTL0"` (magic identifier)
- Bytes 4-7: Tuner type (big-endian u32, typically 5 for R820T)
- Bytes 8-11: Maximum gain value (big-endian u32, typically 0x1d)

In multi-client serve and proxy modes, the magic packet is cached and sent to every connected client (master and slaves alike).

### Encryption Handshake

When `--key` or `--key-file` is provided in proxy mode, after chain detection the two peers perform a nonce exchange: each generates a random 12-byte nonce via ChaCha20's `rand::thread_rng`, sends it, and receives the peer's nonce. Subsequent IQ data is encrypted/decrypted using ChaCha20 with the shared key and exchanged nonces.

## Migration Guide

### From v0.8.x to v0.9.0

1. The binary name changed from `rtltcp` to `rtltcp2026` in v0.8.0. Update your service files and scripts.
2. `--port` is now aliased to `--master-port`. The short flag `-p` still works. Help output shows `--master-port`.
3. **New flags**: `--mode`, `--slave-port`, `--max-slaves`, `--upstream`, `--key`, `--key-file`.
4. v0.8.x invocations work unchanged: `rtltcp2026` still runs single-client serve mode on port 1234.
5. systemd socket activation only activates the master port. Add `--slave-port` to `ExecStart` for multi-client setups.
6. ChaCha20 (`chacha20` crate), `tokio` (sync feature), `rand`, and `hex` are new dependencies.
7. 170+ tests (up from 150+).

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
