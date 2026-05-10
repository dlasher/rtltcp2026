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

## Installation

### Download the latest binary release

Download the [latest release](https://github.com/FirebirdRender/rtltcp/releases) for your platform:

```bash
# x86_64 Linux
wget https://github.com/FirebirdRender/rtltcp/releases/download/0.2.1/rtltcp-linux-x86_64.tar.gz
tar xzf rtltcp-linux-x86_64.tar.gz
sudo mv rtltcp /usr/local/bin/
chmod +x /usr/local/bin/rtltcp

# ARM64 (aarch64)
wget https://github.com/FirebirdRender/rtltcp/releases/download/0.2.1/rtltcp-linux-arm64.tar.gz
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
rtltcp 0.2.1
an I/Q spectrum server for RTL2832 based DVB-T receivers

USAGE:
    rtltcp [OPTIONS]

OPTIONS:
    -a, --address <ADDRESS>     listen address [default: 127.0.0.1]
    -p, --port <PORT>           listen port [default: 1234]
    -d, --device-index <INDEX>  device index [default: 0]
    -b, --buffers <COUNT>       number of decoding buffers [default: 15, range: 1-32]
    -s, --tcp-buffers <SIZE>    tcp sending buffer size (bytes) [default: 512000, range: 1-10485760]
    --read-timeout <SECONDS>    socket read timeout [default: 30]
    --write-timeout <SECONDS>   socket write timeout [default: 30]
    -h, --help                  Print help
```

### Connect with an SDR client

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

[Service]
Type=notify
ExecStart=/usr/local/bin/rtltcp
TimeoutStopSec=5
```

Create `/etc/systemd/system/rtltcp.socket`:

```ini
[Unit]
Description=RTL TCP Socket
PartOf=rtltcp.service

[Socket]
ListenStream=[::]:1234

[Install]
WantedBy=sockets.target
```

Enable and start:

```bash
sudo systemctl enable rtltcp.socket
sudo systemctl start rtltcp.socket
```

## Protocol Commands

| Command | Code | Description |
|---------|------|-------------|
| Set Frequency | 0x01 | Set center frequency (Hz) |
| Set Sample Rate | 0x02 | Set sample rate (Hz) |
| Set Gain Mode | 0x03 | 0 = auto (AGC on), non-zero = manual |
| Set Tuner Gain | 0x04 | Set manual gain (dB * 10) |
| Set PPM | 0x05 | Set frequency correction (ppm) |
| Set AGC | 0x08 | 1 = AGC on, 0 = AGC off |

## Building

```bash
# Development build
cargo build

# Release build (with optimizations)
cargo build --release

# Run tests
cargo test --all-features

# Run with logging
RUST_LOG=debug cargo run -- --port 1234
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