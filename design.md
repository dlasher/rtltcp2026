# Design: RTL_TCP Capture & Replay Tools

Version: 1.0
Date: 2026-05-22
Depends on: requirements.md v1.0

---

## 1. Capture File Format

The capture file is a binary format consisting of a fixed-size header followed by a sequence of variable-length chunks.

### Header (variable length, min 12 bytes)

```
Offset  Size  Field          Description
0       4     magic          Magic bytes b"RTLX" (0x52544c58)
4       4     version        Format version as u32 LE (currently 1)
8       4     magic_len      Length of magic_payload as u32 LE
12      N     magic_payload  The raw 12-byte RTL_TCP magic packet from the server
```

### Chunk (variable length, 12 bytes overhead + data)

Each chunk represents one `read()` from the TCP socket:

```
Offset  Size  Field         Description
0       8     timestamp_ns  Monotonic timestamp (CLOCK_MONOTONIC) as u64 LE
8       4     data_len      Length of data payload as u32 LE
12      N     data          Raw IQ data bytes
```

**Design rationale:**
- Monotonic clock avoids NTP adjustments causing non-monotonic timestamps
- Absolute (not delta) timestamps make seeking and streaming from disk simpler
- 12 bytes overhead per chunk is negligible at typical 512 KB read sizes (< 0.003%)

---

## 2. File Structure

```
src/
  capture/
    mod.rs        — CaptureHeader, CaptureChunk types, read/write functions
  bin/
    capture.rs    — rtltcp-capture binary entry point
    replay.rs     — rtltcp-replay binary entry point
```

### Module: `src/capture/mod.rs` [REQ-008]

Exports:

```rust
pub const CAPTURE_MAGIC: &[u8; 4] = b"RTLX";
pub const CAPTURE_VERSION: u32 = 1;

pub struct CaptureHeader {
    pub magic_payload: Vec<u8>,  // the 12-byte RTL_TCP magic packet
}

pub struct CaptureChunk {
    pub timestamp_ns: u64,
    pub data: Vec<u8>,
}

pub fn write_header<W: Write + Seek>(writer: &mut W, header: &CaptureHeader) -> io::Result<()>
pub fn read_header<R: Read>(reader: &mut R) -> io::Result<CaptureHeader>
pub fn write_chunk<W: Write>(writer: &mut W, chunk: &CaptureChunk) -> io::Result<()>
pub fn read_chunk<R: Read>(reader: &mut R) -> io::Result<Option<CaptureChunk>>
```

- `write_header` uses `Seek` to write the header at position 0 after the magic payload is known
- `read_chunk` returns `None` on EOF (partial read = corruption error)
- All numeric fields are little-endian for portability

---

## 3. Binary: `rtltcp-capture` [REQ-001..REQ-007]

### CLI Arguments

| Argument | Type | Default | Description |
|---|---|---|---|
| `OUTPUT` | positional | required | Path to output capture file |
| `--host` | string | `127.0.0.1` | RTL_TCP server host |
| `--port` | u16 | `1234` | RTL_TCP server port |
| `--duration` | u64 | `10` | Capture duration in seconds |
| `--timeout` | u64 | `1` | Socket read timeout in seconds |
| `--buffer-mem` | u64 | `67108864` | Max in-memory buffer before flush to disk |

### Flow

```
parse args
open output file with write+seek
connect TcpStream(host, port)
read 12-byte magic packet from server
write header at position 0 (seek back after magic known)
start monotonic clock
loop:
    read from socket
    append timestamp_ns + data_len + data to memory buffer
    if buffer size > buffer_mem:
        write buffer to file, clear buffer
    if elapsed > duration: break
on Ctrl-C (SIGINT):
    flush buffer to file
    close file
    print stats: total bytes, elapsed time, file size
```

**Memory buffer design:** Accumulate raw chunk bytes (timestamp_ns + data_len + data) as a single `Vec<u8>`. No per-chunk allocation overhead. On flush, `write_all()` the buffer to file.

**Duration check:** Check elapsed time after each `read()`. The read timeout (`--timeout`, default 1s) ensures we unblock periodically to check the clock without losing data to WouldBlock loops.

### Threading

Single-threaded. The read timeout handles periodic duration checks. Ctrl-C handled via `ctrlc` crate (already a dependency).

### Error handling

All errors propagate as `String` via `Box<dyn Error>` or a simple `Result<(), String>` exit pattern. No need for the full `RtlTcpError` enum from the library — binaries use `eprintln!` and exit with code 1.

---

## 4. Binary: `rtltcp-replay` [REQ-009..REQ-015]

### CLI Arguments

| Argument | Type | Default | Description |
|---|---|---|---|
| `INPUT` | positional | required | Path to capture file |
| `--port` | u16 | `1234` | Listen port |
| `--bind` | string | `127.0.0.1` | Listen address |
| `--speed` | f64 | `1.0` | Playback speed multiplier (0 = as-fast-as-possible) |
| `--loop` | bool | false | Restart from beginning when exhausted |

### Flow

```
parse args
open capture file
read header (12 bytes + magic_payload)
bind TcpListener(port)
accept one client
send magic_payload to client (reuse the exact bytes from the capture)
start command_reader thread (reads 5-byte commands, logs them)
initialize prev_timestamp = None
loop:
    read chunk from file (timestamp_ns, data_len, data)
    if EOF:
        if --loop: seek to first chunk, continue
        else: break
    if speed > 0 and prev_timestamp is Some:
        delta = (timestamp_ns - prev_timestamp) as f64 / speed
        sleep(delta_ns)
    write data to client
    prev_timestamp = Some(timestamp_ns)
on client write error or disconnect:
    signal command_reader thread to stop
    close connections
```

### Threading

Two threads:
1. **Main thread** — accepts client, sends magic, streams chunks from disk
2. **Command reader thread** — reads 5-byte commands from client, logs them

The command reader thread is stopped via an `Arc<AtomicBool>` flag when the main thread detects client disconnect.

### Command logging [REQ-013]

```
info!("client command: SET_FREQUENCY freq=100000000")
info!("client command: SET_SAMPLE_RATE rate=2048000")
info!("client command: CMD_0xFF payload=01020304")
info!("client command: UNKNOWN payload=deadbeef")
```

The command parser matches byte 0 against `control::CMD_*` constants. The `control` module is already public via `lib.rs`.

### Speed handling [REQ-011]

- `--speed 0`: No inter-chunk delay. Tight loop: read chunk, write to client.
- `--speed 1.0`: Sleep for `chunk.timestamp_ns - prev_timestamp_ns`.
- `--speed 2.0`: Sleep for `(chunk.timestamp_ns - prev_timestamp_ns) / 2`.
- The first chunk has no `prev_timestamp` — sent immediately.

### Loop mode [REQ-012]

When `--loop` is set and EOF is reached, seek back to the position immediately after the header. The first chunk's timestamp becomes the new "first" for timing purposes (sent immediately).

---

## 5. Cargo.toml Changes

```toml
[[bin]]
name = "rtltcp-capture"
path = "src/bin/capture.rs"

[[bin]]
name = "rtltcp-replay"
path = "src/bin/replay.rs"
```

No new dependencies required. Both binaries use:
- `clap` (already in workspace)
- `tracing` + `tracing-subscriber` (already in workspace)
- `ctrlc` (already in workspace)
- `rtltcp2026` library for shared constants and capture module

The `rtltcp2026` lib crate gains `pub mod capture;` to expose the format module.

---

## 6. REQ Traceability Matrix

| Requirement | Design Element |
|---|---|
| REQ-001 | capture.rs: connect to host:port |
| REQ-002 | capture/mod.rs: write_header | capture.rs: read magic from server |
| REQ-003 | capture/mod.rs: write_chunk | capture.rs: read loop |
| REQ-004 | capture.rs: --duration, elapsed time check |
| REQ-005 | capture.rs: --buffer-mem, flush on threshold |
| REQ-006 | capture.rs: ctrlc handler, flush on signal |
| REQ-007 | capture.rs: print stats on exit |
| REQ-008 | capture/mod.rs: CaptureHeader format |
| REQ-009 | replay.rs: read_header, read_chunk |
| REQ-010 | replay.rs: TcpListener, send magic, stream data |
| REQ-011 | replay.rs: --speed, inter-chunk delay calculation |
| REQ-012 | replay.rs: --loop, seek on EOF |
| REQ-013 | replay.rs: command_reader thread, log command type |
| REQ-014 | replay.rs: AtomicBool shutdown, join threads |
| REQ-015 | replay.rs: read_chunk() per iteration, no preload |
| NFR-001 | Buffer-mem default 64 MB absorbs disk latency |
| NFR-002 | No-copy writes from buffer Vec; --speed 0 tight loop |
| NFR-003 | All results checked with expect/unwrap_or_else + eprintln |

---

## Open Questions

1. **Buffer flush size reporting**: Should the capture tool print individual flush sizes, or just aggregate at the end? (Current design: aggregate only.)
2. **Replay multiple connections**: Replay accepts one client and exits. Should we loop back to `accept()` after disconnect (like serve mode's master reconnection)? Currently: no — single client, single connection.
3. **Clock source for capture timestamps**: Using `CLOCK_MONOTONIC`. On Linux, `std::time::Instant` uses `CLOCK_MONOTONIC`. Is there a need for wall-clock timestamps?
4. **Replay speed precision**: `sleep` uses `std::thread::sleep(Duration::from_nanos(...))`. Nanosecond precision is supported but Linux may round to ~50 µs depending on kernel config. Is that acceptable?
