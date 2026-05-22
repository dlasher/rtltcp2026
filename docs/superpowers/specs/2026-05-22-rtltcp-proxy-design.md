# RTLTCP Multi-Client Proxy Design

**Date:** 2026-05-22
**Status:** Draft (v2.0 — updated after council review)
**Version:** 2.0

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

### New Dependency

**tokio** (with `sync` feature) is added for its bounded broadcast channel (`tokio::sync::broadcast`). Only the `sync` feature is needed at this stage; no async runtime is required. The broadcast channel drops data for lagging receivers automatically and supports bounded capacity. Other parts of the codebase remain std-thread-based. Future work (e.g., async I/O for the upstream connection) can use more of tokio.

### CLI Interface (Flat Flags)

```
rtltcp --mode serve [--device-index <N>] [--master-port <P>] --slave-port <P> [--max-slaves <N>]
rtltcp --mode proxy --upstream <host:port> [--master-port <P>] [--slave-port <P>] [--max-slaves <N>] [--key <KEY>] [--key-file <PATH>]
```

Existing flags (`--address`, `--port`, `--buffers`, `--tcp-buffers`, `--read-timeout`, `--write-timeout`, `--whitelist`) remain supported with the same defaults.

- `--mode serve` is the default (no `--mode` flag → legacy single-client behavior).
- `--port` becomes an alias for `--master-port` for backward compatibility.
- `--address` applies to both master and slave ports — both listen on the same address.
- `--mode proxy` requires `--upstream` (validated by clap).
- `--upstream` requires `--mode proxy` (validated by clap).
- `--key` and `--key-file` are mutually exclusive (clap `conflicts_with`).
- `--upstream <host:port>` accepts IPv4 as `host:port`, IPv6 as `[host]:port`.

### Port Roles

- **Master port** (`--master-port`, default 1234): accepts a single connection. That client is the "driver" — its control commands (frequency, sample rate, gain mode, tuner gain, PPM, AGC) are applied to the device (serve mode) or forwarded to the upstream (proxy mode).
- **Slave port** (`--slave-port`, default 1235): accepts multiple connections (up to `--max-slaves`, default 10). Each slave receives the full IQ stream but their control commands are silently consumed (read from socket and discarded). Slaves cannot distinguish themselves from the master. Slave port is only opened when `--slave-port` is explicitly provided.
- **Single-port fallback:** If `--slave-port` is not specified, the binary behaves as it does today — one client, full control.

### systemd Socket Activation

Socket activation applies only to the master port. Slave ports are opened by the process at runtime (no systemd activation for slaves). When using systemd activation:

- systemd socket file listens on `--master-port`
- On socket activation, the process opens the slave port itself (if `--slave-port` is set)
- The slave port is an ephemeral listener created by the process, not managed by systemd

## Data Flow

### Broadcast Channel Architecture

```
[USB Device / Upstream TCP] → [Reader Thread] → [tokio::sync::broadcast<Vec<u8>>]
                                                         │
                          ┌───────────────────────────────┼───────────────────┐
                          │                               │                   │
                     [Client 1 Writer]              [Client 2 Writer]   [Client N Writer]
                          │                               │                   │
                     [TCP Socket]                    [TCP Socket]        [TCP Socket]
```

1. **Reader thread:** Reads IQ data from the input source (USB via `read_async` callback, or upstream TCP via blocking reads). Each buffer is pushed into the broadcast channel via `send()`.
2. **Writer threads:** Each connected client gets a writer thread. The thread subscribes to the broadcast channel via `subscribe()`, receives each IQ buffer, and writes it to the client's TCP socket.
3. **Slow clients:** `tokio::sync::broadcast` drops data for slow consumers automatically. A lagging writer's receiver returns `RecvError::Lagged(n)` when it falls behind. The writer logs the dropped count and writes the next buffer — it never blocks the reader or other writers.
4. **Memory:** The broadcast channel has a bounded buffer (configurable, default **64** IQ buffers). At 8 MB/s (2 MS/s, 8-bit IQ), 64 buffers ≈ 4 seconds of latency tolerance. This bounds memory at ~4 MB per subscriber regardless of how many clients are connected.

### read_async Callback Bridge (Serve Mode)

In serve mode, `rtlsdr_mt::reader.read_async()` invokes a callback on a libusb-internal thread. This callback pushes each IQ buffer to the broadcast channel:

```rust
let tx = broadcast_tx.clone();
reader.read_async(buffers, 0, move |bytes| {
    let _ = tx.send(bytes.to_vec());  // non-blocking, bounded channel
});
```

The callback must remain non-blocking (no I/O, no long-held locks). Pushing to a bounded `tokio::sync::broadcast` with `send()` satisfies this — it's O(1) and never blocks the caller on a full channel (oldest item is dropped).

In proxy mode, the upstream TCP reader runs in a dedicated thread with blocking reads — no callback to bridge.

### Connection Protocol (Slave Handshake)

On slave connect, the server sends the cached 12-byte magic packet — identical to what a direct rtltcp connection would send. The slave sees the exact same handshake as it would from a real SDR.

In serve mode, the magic packet values (tuner type, max gain) are queried from the actual device handle at startup, not hardcoded. In proxy mode, the upstream's magic packet bytes are received and cached.

### Connection Protocol (Proxy Mode)

1. Bind master and slave ports. Slave port defers listing until upstream is established.
2. Wait for a master connection.
3. On master connect: open outbound TCP to `--upstream`.
4. Read 12-byte magic packet from upstream, cache it for slave handshakes.
5. Send `0xF0` chain detection probe. Wait up to 500ms for response.
   - If `0xF0` ACK received → chain mode (potentially encrypted).
   - If timeout or no response → plain proxy mode.
6. Start upstream reader thread → broadcast channel.
7. List slave port. When slaves connect: send cached magic packet, spawn writer thread.

A master command from the proxy's master is not forwarded upstream until the `0xF0` handshake completes (or times out). This prevents racing master commands against the chain detection.

## Control Flow

### Master Commands (All Modes)

Master's 5-byte commands (frequency 0x01, sample rate 0x02, gain mode 0x03, tuner gain 0x04, PPM 0x05, AGC 0x08) are:

- **Serve mode:** validated locally, then applied to the `rtlsdr_mt` device control handle (existing behavior).
- **Proxy mode:** validated locally, then forwarded to the upstream TCP socket as-is. Only valid commands are forwarded — invalid ones are rejected with a warning (same as serve mode). This prevents noisy invalid traffic on the upstream connection.
- **Chain mode:** validated locally, then forwarded through the encrypted stream.

The local rate limiter applies to the master connection only (slaves have commands silently consumed).

### Slave Commands (All Modes)

Slave's commands are read from the socket and silently consumed. The protocol has no response/ack mechanism — commands are fire-and-forget. The slave thread:

1. Reads the 5-byte command header
2. Optionally reads the 4-byte payload (already part of the 5 bytes)
3. Logs at trace level and discards
4. The slave cannot detect it isn't controlling the device

A per-slave rate limiter applies to prevent a malicious slave from exhausting resources by flooding commands that are just discarded.

### Chain Detection

When a downstream proxy connects to an upstream proxy (or rtltcp), it sends a standard 5-byte command after receiving the magic packet:

- Opcode: `0xF0` (unused in the rtl_tcp protocol)
- Payload: 4-byte magic value `0x50524F58` (`"PROX"` as u32 BE)

If the upstream recognizes this opcode, it responds with a matching 5-byte acknowledgment: `[0xF0, 0x00, 0x00, 0x00, 0x00]` (zero payload). Both sides then proceed to encryption negotiation.

If the upstream does not recognize `0xF0` (i.e., it's a standard rtltcp server), it logs an unknown-command warning (existing behavior, line 502-506 of main.rs) and continues. The proxy detects the absence of an `0xF0` response via a **500ms timeout** and falls back to plain proxy mode.

Non-proxy clients that encounter `0xF0` similarly just log an unknown command and continue — there is no protocol-level error that terminates the connection.

### Graceful Shutdown with Multi-Client

The current signal handler (lines 339-357 of main.rs) shuts down a single TCP stream to unblock the control thread. With N writer threads + 1 reader thread + 1 control thread, the shutdown sequence is:

1. Signal handler sets `should_exit` flag (AtomicBool).
2. Signal handler closes the master TCP stream (current behavior) — unblocks the control thread.
3. Signal handler closes all slave TCP streams — unblocks each writer thread.
4. Writer threads check `should_exit` after write errors and terminate.
5. Reader thread checks `should_exit` and stops reading.
6. When all threads terminate, process exits normally.

All slave TCP streams are tracked in an `Arc<Mutex<Vec<TcpStream>>>` that the signal handler iterates on shutdown.

## Encryption (Proxy-to-Proxy)

### Cipher

**ChaCha20 (IETF variant)** via the `chacha20` crate, with 12-byte (96-bit) nonces. Chosen for:

- ~3 cycles/byte on ARM, well under 1% CPU on a Pi 4 at RTL-SDR bandwidths (~2 MB/s)
- Constant-time, no padding overhead
- Well-reviewed, standard Rust implementation

### Key Provision

Two methods, mutually exclusive (clap `conflicts_with`):

- `--key-file <PATH>`: read 32-byte raw key from file
- `--key <HEX>`: hex-encoded 32-byte key (less secure — visible in process list)

### Negotiation

1. After `0xF0` chain detection handshake, both sides generate a random 12-byte nonce.
2. Downstream sends its nonce to upstream (12 raw bytes).
3. Upstream sends its nonce to downstream (12 raw bytes).
4. Both sides initialize ChaCha20 state machines:
   - Encrypt outbound: `ChaCha20::new(&shared_key, &nonce_sent)`
   - Decrypt inbound: `ChaCha20::new(&shared_key, &nonce_received)`
5. All subsequent traffic on the proxy-proxy connection is encrypted.

Each direction uses a different nonce, ensuring bi-directional security.

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
| Master disconnects, no slaves | Tear down upstream (USB or TCP). Wait for a new master connection. The new master must re-send all desired settings — no previous configuration is cached. |
| Slave disconnects | Remove subscriber from broadcast. No effect on master or other slaves. |
| Upstream disconnect (proxy) | IQ stream stops. Slave port refuses connections. Master can reconnect upstream by re-establishing (if the upstream comes back). |
| USB device error (serve) | Same as upstream disconnect — no more IQ, slave port refuses connections. |
| All slaves disconnect, master stays | Reader thread continues — IQ stream stays active. The master may still be controlling the device. |
| Process shutdown (SIGTERM/SIGINT) | All slave TCP streams closed immediately. `should_exit` flag set. All threads terminate. |
| Slave port bind fails | Hard error — process exits with error message. No silent degradation. |
| Max slaves reached | Accept loop rejects new slave connections (connection reset). |

## Testing Strategy

### Unit Tests (Inline)

- Broadcast channel behavior (subscriber add/remove, lag handling)
- Master/slave command routing
- Command validation (existing, extended for proxy)
- Rate limiting (existing, per-slave variant)
- ChaCha20 encryption round-trip
- Proxy-proxy chain detection handshake (`0xF0` negotiation + timeout)
- Magic packet construction and caching
- EncryptedStream read/write round-trip (ChaCha20 wrap/unwrap)

### Integration Tests (Written Alongside Each Step)

- After stream module: spawn server, connect one client, verify IQ data flows.
- After multi-client serve mode: connect master + slaves, verify all receive same IQ, slave commands silently consumed.
- After proxy mode: real rtltcp server upstream → proxy → master + slaves.
- After chain detection: proxy-proxy chain detection with and without 0xF0 support.
- After encryption: encrypted proxy-proxy chain, data integrity check.
- Master disconnect → slaves continue until last leaves; master reconnect flow.
- Slave port refuses connections when no master is present.
- Max-slaves limit enforced.
- Graceful shutdown with multiple slaves connected.

### Hardware Tests (Feature-gated)

Behind `#[cfg(feature = "hardware-tests")]` or `#[ignore]`:

- Real RTL-SDR stick connected, run serve mode with master + slaves
- Real RTL-SDR with upstream proxy connecting to it
- Verify real IQ data reaches all clients

These must not run in CI (require physical hardware).

### Module Structure

Current monolithic `main.rs` (890 lines) is split into modules:

```
src/
├── main.rs         # CLI args parsing, dispatch to mode
├── error.rs        # Existing RtlTcpError
├── control.rs      # Command parsing, validation, master/slave routing
├── stream.rs       # Broadcast channel, reader thread, writer threads
├── proxy.rs        # Upstream connection mgmt, chain detection, negotiation
├── encryption.rs   # EncryptedStream, key management, ChaCha20 setup
└── tests/          # Integration tests
    ├── mod.rs
    ├── serve.rs
    ├── proxy.rs
    └── chain.rs
```

Each module has its own `#[cfg(test)] mod tests { ... }` block for unit tests.

### Buffer Allocation

Each IQ buffer from `read_async` is pushed as `Vec<u8>` (owned, heap-allocated). At 64 buffer capacity and 8 MB/s, this allocates ~4-8 MB per subscriber. A simple `Vec<Vec<u8>>` buffer pool could reduce allocator churn if profiling shows it's a bottleneck, but the initial implementation uses fresh allocations.

## Implementation Order

The build proceeds in this order, with integration tests written alongside each feature step:

| Order | Step | Description | Tests produced |
|-------|------|-------------|----------------|
| 1a | CLI args update | Add `--mode`, `--master-port`, `--slave-port`, `--max-slaves`, `--upstream`, `--key`, `--key-file`. Add clap `requires_if` / `conflicts_with`. Make `--port` alias for `--master-port`. | — |
| 1b | Module extraction | Extract `error.rs` (trivial), `control.rs` (commands, validation, rate limiting, master/slave routing). | Unit tests for control module |
| 2 | Stream module | `tokio::sync::broadcast`-based broadcast channel. Reader thread abstraction (trait for USB callback vs TCP blocking read). Writer thread per client with subscription. Slow-client lag handling. Graceful shutdown coordination. | Unit + integration tests for broadcast/spawn |
| 3 | Multi-client serve mode | Wire stream module into USB mode. Accept master + slave connections. `read_async` callback pushes to broadcast. Master control commands → device. Slave commands silently consumed. | Integration tests: master + slaves, same IQ, silent slave commands |
| 4 | Proxy mode | TCP upstream connection. Chain detection `0xF0` with 500ms timeout. Upstream reader thread → broadcast. Control forwarding. Magic packet caching from upstream. Slave port deferred listing. | Integration tests: upstream → proxy → clients |
| 5 | Encryption | `EncryptedStream` wrapper. ChaCha20 nonce exchange. Key CLI flags. Encrypted proxy-proxy sessions. | Integration tests: encrypted chain, data integrity |
| 6 | Hardware tests | Feature-gated real device tests. | End-to-end with USB stick |

### Step Dependencies

```
1a (CLI args) ──→ 1b (modules) ──→ 2 (stream) ──→ 3 (serve) ──→ 4 (proxy) ──→ 5 (encryption)
                                                                                     │
                                                                               6 (hardware)
```

No step depends on a later step. Each step is testable independently.

## Out of Scope

- Authentication beyond shared-key (no TLS, no PKI).
- Protocol extensions beyond `0xF0` chain detection.
- Client software modifications — zero expectation clients are aware of proxy mode.
- Dynamic reconfiguration (adding slaves without CLI restart).
- Async runtime overhaul beyond `tokio::sync::broadcast`.
