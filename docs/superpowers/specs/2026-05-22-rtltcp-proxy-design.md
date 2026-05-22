# RTLTCP Multi-Client Proxy Design

**Date:** 2026-05-22
**Status:** Draft
**Version:** 1.0

## Overview

RTL-SDR sticks are receive-only — they can only produce one IQ stream at a time configured for one center frequency, sample rate, gain, and PPM correction. This design extends the rtltcp binary to allow multiple consumers to share a single SDR feed, with two operating modes:

1. **Serve mode** (USB SDR input, multi-client fan-out) — existing behavior extended
2. **Proxy mode** (TCP upstream input, multi-client fan-out) — connects to an upstream rtltcp server instead of USB

A third variant, **chain mode**, allows proxies to connect to other proxies, with optional ChaCha20 encryption between proxy peers.

## Architecture

### Operating Modes

| Mode | Input Source | Control | Fan-out |
|------|-------------|---------|---------|
| Serve | USB device | Hardware | Local clients |
| Proxy | Upstream TCP | Forwarded to upstream | Local clients |
| Chain | Upstream proxy TCP | Forwarded through chain | Local clients |

### Binary, Not Daemon

The existing single-binary approach is extended. All modes live in the same binary, selected at runtime via CLI flags — no separate proxy binary.

### CLI Interface (Flat Flags)

```
rtltcp --mode serve [--device-index <N>] [--master-port <P>] [--slave-port <P>] [--max-slaves <N>]
rtltcp --mode proxy --upstream <host:port> [--master-port <P>] [--slave-port <P>] [--max-slaves <N>] [--key <KEY>] [--key-file <PATH>]
```

Existing flags (`--address`, `--port`, `--buffers`, `--tcp-buffers`, `--read-timeout`, `--write-timeout`, `--whitelist`) remain supported with the same defaults.

The `--port` flag becomes an alias for `--master-port` for backward compatibility. The `--address` flag applies to both master and slave ports — both listen on the same address.

### Port Roles

- **Master port** (`--master-port`, default 1234): accepts a single connection. That client is the "driver" — its control commands (frequency, sample rate, gain mode, tuner gain, PPM, AGC) are applied to the device (serve mode) or forwarded to the upstream (proxy mode).
- **Slave port** (`--slave-port`): accepts multiple connections (up to `--max-slaves`, default 10). Each slave receives the full IQ stream but their control commands are silently consumed (read from socket and discarded). Slaves cannot distinguish themselves from the master.
- **Single-port fallback:** If `--slave-port` is not specified, the binary behaves as it does today — one client, full control.

## Data Flow

### Broadcast Channel Architecture

```
[USB Device / Upstream TCP] → [Reader Thread] → [Broadcast<Vec<u8>>]
                                                      │
                          ┌───────────────────────────┼───────────────────┐
                          │                           │                   │
                     [Client 1 Writer]          [Client 2 Writer]   [Client N Writer]
                          │                           │                   │
                     [TCP Socket]               [TCP Socket]        [TCP Socket]
```

1. **Reader thread:** Reads IQ data from the input source (USB via `read_async`, or upstream TCP via blocking reads). Each buffer is pushed into a shared broadcast channel.
2. **Writer threads:** Each connected client gets a writer thread. The thread subscribes to the broadcast channel, receives each IQ buffer, and writes it to the client's TCP socket.
3. **Slow clients:** A custom broadcast channel backed by a bounded ring buffer drops data for slow consumers. Each writer subscribes with its own slot; if slots overflow (consumer too slow), the oldest buffer is dropped. A lagging writer drops buffers rather than blocking the reader or other writers.
4. **Memory:** The broadcast channel has a bounded buffer (configurable, default ~256 IQ buffers). This bounds the memory cost regardless of how many clients are connected.

### Connection Protocol (Slave Handshake)

On slave connect, the server sends the cached 12-byte magic packet (`RTL0` + tuner type 5 + max gain 0x1d) — identical to what a direct rtltcp connection would send. The slave sees the exact same handshake as it would from a real SDR.

### Connection Protocol (Proxy Mode)

1. Bind master and slave ports
2. Wait for a master connection
3. On master connect: open outbound TCP to `--upstream`
4. Read 12-byte magic packet from upstream, cache it for slave handshakes
5. Start upstream reader thread → broadcast channel
6. Master's 5-byte control commands are read by a control thread and forwarded to upstream
7. When slaves connect: send cached magic packet, spawn writer thread

## Control Flow

### Master Commands (All Modes)

Master's 5-byte commands (frequency 0x01, sample rate 0x02, gain mode 0x03, tuner gain 0x04, PPM 0x05, AGC 0x08) are:
- **Serve mode:** forwarded to the `rtlsdr_mt` device control handle (existing behavior)
- **Proxy mode:** forwarded to the upstream TCP socket as-is
- **Chain mode:** forwarded through the encrypted stream to the upstream proxy → eventually to hardware

### Slave Commands (All Modes)

Slave's commands are read from the socket and silently consumed. The protocol has no response/ack mechanism — commands are fire-and-forget. The slave thread simply reads the 5-byte header and discards it, logging a debug trace. The slave cannot detect it isn't controlling the device.

### Chain Detection

When a downstream proxy connects to an upstream proxy (or rtltcp), it sends a standard 5-byte command after receiving the magic packet:

- Opcode: `0xF0` (unused in the rtl_tcp protocol)
- Payload: 4-byte magic value `0x50524F58` (`"PROX"` as u32 BE)

If the upstream recognizes this opcode, it responds with a matching `0xF0` acknowledgment. Both sides then proceed to encryption negotiation.

If the upstream does not recognize `0xF0` (i.e., it's a standard rtltcp server), the client sees an unknown-command warning and continues as a normal client. Non-proxy clients that encounter `0xF0` similarly just log an unknown command and continue — there is no protocol-level error that terminates the connection.

## Encryption (Proxy-to-Proxy)

### Cipher

**ChaCha20** via the `chacha20` crate. Chosen for:
- ~3 cycles/byte on ARM, well under 1% CPU on a Pi 4 at RTL-SDR bandwidths (~2 MB/s)
- Constant-time, no padding overhead
- Well-reviewed, standard Rust implementation

### Key Provision

Two methods, mutually exclusive:
- `--key-file <PATH>`: read 32-byte raw key from file
- `--key <HEX>`: hex-encoded 32-byte key (less secure — visible in process list)

### Negotiation

1. After `0xF0` chain detection handshake, both sides generate a random 12-byte nonce
2. Downstream sends its nonce to upstream
3. Upstream sends its nonce to downstream
4. Both sides initialize ChaCha20 state machines: `ChaCha20::new(&shared_key, &nonce_sent)` for encrypting outbound, `ChaCha20::new(&shared_key, &nonce_received)` for decrypting inbound
5. All subsequent traffic on the proxy-proxy connection is encrypted

### EncryptedStream Wrapper

A thin struct wrapping `TcpStream`:

```rust
struct EncryptedStream {
    inner: TcpStream,
    enc: ChaCha20,  // outbound keystream
    dec: ChaCha20,  // inbound keystream
}
```

Implements `Read` (decrypt on read) and `Write` (encrypt on write). The rest of the proxy code operates on `&mut dyn Read + Write` and is unaware of encryption.

### Fallback

If no `--key` or `--key-file` is specified, proxy-proxy connections use the `0xF0` handshake but skip encryption. The connection is plain TCP.

## Error Handling & Disconnect Behavior

| Event | Behavior |
|-------|----------|
| Master disconnects, slaves exist | Keep streaming with current settings. No further control changes. When last slave disconnects, tear down. |
| Master disconnects, no slaves | Tear down upstream (USB or TCP). Wait for a new master connection. |
| Slave disconnects | Remove subscriber from broadcast. No effect on master or other slaves. |
| Upstream disconnect (proxy) | IQ stream stops. Slave ports refuse connections until master re-establishes. |
| USB device error (serve) | Same as upstream disconnect — no more IQ, slaves refused until master re-establishes. |

## Testing Strategy

### Unit Tests (Inline)

- Broadcast channel behavior (subscriber add/remove, lag handling)
- Master/slave command routing
- Command validation (existing, extended for proxy)
- Rate limiting (existing)
- ChaCha20 encryption round-trip
- Proxy-proxy chain detection handshake (`0xF0` negotiation)
- Magic packet construction and caching

### Integration Tests (`tests/` directory)

- Spawn server, connect master and slaves, verify slave IQ matches master
- Master sets frequency/gain/ppm → slaves see same output parameters
- Slave commands are silently consumed (no observable effect)
- Master disconnect → slaves continue until last leaves, then process terminates
- Proxy mode: real rtltcp server upstream → proxy → master + slaves
- Proxy-proxy chain: two proxies connecting, with and without encryption
- Slave port refuses connections when no master is present
- Max-slaves limit enforced

### Hardware Tests (Feature-gated)

Behind `--features hardware-tests` or `#[ignore]`:
- Real RTL-SDR stick connected, run serve mode with master + slaves
- Real RTL-SDR with upstream proxy connecting to it
- Verify real IQ data reaches all clients

These must not run in CI (require physical hardware).

### Module Refactor

Current monolithic `main.rs` (890 lines) is split into modules:

```
src/
├── main.rs         # CLI args parsing, dispatch to mode
├── error.rs        # Existing RtlTcpError
├── control.rs      # Command parsing, validation, master/slave routing
├── stream.rs       # Broadcast channel, reader thread, writer threads
├── proxy.rs        # Upstream connection mgmt, chain detection, negotiation
├── encryption.rs   # EncryptedStream, key management, ChaCha20 setup
└── tests.rs        # Integration tests (or tests/ directory)
```

Each module has its own `#[cfg(test)] mod tests { ... }` block for unit tests.

## Implementation Order

The build should proceed in this order:

1. **Module refactor** — extract error handling, then control module (commands, validation, rate limiting)
2. **Stream module** — broadcast channel, reader/writer thread pattern
3. **Multi-client serve mode** — extend existing USB mode to accept master + slave connections
4. **Proxy mode** — TCP upstream connection, control forwarding, slave management
5. **Chain detection** — `0xF0` opcode handshake, proxy-proxy detection
6. **Encryption** — `EncryptedStream` wrapper, nonce exchange negotiation, key CLI flags
7. **Integration tests** — TCP-based multi-client scenarios
8. **Hardware tests** — real device, feature-gated

## Out of Scope

- Authentication beyond shared-key (no TLS, no PKI)
- Protocol extensions beyond `0xF0` chain detection
- Client software modifications — zero expectation clients are aware of proxy mode
- Dynamic reconfiguration (adding slaves without CLI restart)
