# Requirements: RTL_TCP Capture & Replay Tools

Version: 1.0
Date: 2026-05-22

## Overview

Two companion CLI tools for the rtltcp2026 project:
- **rtltcp-capture** — connects to any RTL_TCP server and saves raw IQ data with timing metadata to a file
- **rtltcp-replay** — serves as an RTL_TCP server reading from a previously captured file, streaming data to a client at configurable speed

These tools enable offline debugging, test repeatability, and stress testing without requiring a live SDR device.

---

## Functional Requirements

### REQ-001: Capture connects to RTL_TCP server
The capture tool shall connect to any RTL_TCP-compatible server at a user-specified host and port.
- **Acceptance:** Connection succeeds against a live rtltcp2026 server (serve mode slave port, proxy mode slave port, or single-client serve).

### REQ-002: Capture reads magic packet
Upon connection, the capture tool shall read the 12-byte RTL_TCP magic packet from the server and store it in the capture file header.
- **Acceptance:** Captured file header contains the exact magic packet bytes received from the server.

### REQ-003: Capture writes timestamped chunks
The capture tool shall read raw IQ data from the socket in a loop and write each buffer as a chunk with a monotonic timestamp to the capture file.
- **Acceptance:** File contains sequential chunks, each with a non-decreasing `timestamp_ns` and the exact bytes received.

### REQ-004: Capture supports configurable duration
The capture tool shall accept a `--duration` argument (in seconds, default 10) and stop capture after that duration elapses.
- **Acceptance:** Running `rtltcp-capture --duration 5` terminates after approximately 5 seconds.

### REQ-005: Capture supports configurable buffer memory
The capture tool shall accumulate chunks in memory up to a `--buffer-mem` threshold (bytes, default 64 MB), then flush to disk in bulk.
- **Acceptance:** A capture producing 200 MB of data with `--buffer-mem 67108864` results in 3–4 flush operations (not one chunk per read).

### REQ-006: Capture flushes on Ctrl-C
When interrupted by Ctrl-C, the capture tool shall flush any remaining in-memory buffer to the output file and exit cleanly.
- **Acceptance:** `^C` during capture produces a valid, readable capture file containing all data collected up to that point.

### REQ-007: Capture reports stats on exit
On completion, the capture tool shall print total bytes captured, real elapsed time, and file size.
- **Acceptance:** Exit log shows non-zero values for all three metrics.

### REQ-008: Capture file format is self-describing
The capture file shall begin with a header containing magic bytes (`RTLX`), format version, and the server's magic packet. Chunks follow in sequence.
- **Acceptance:** `xxd capture.bin | head -1` shows `52544c58` (`RTLX`).

### REQ-009: Replay reads capture file
The replay tool shall open a capture file, parse its header, and stream chunks from disk on demand.
- **Acceptance:** Replay accepts any file produced by `rtltcp-capture`.

### REQ-010: Replay serves RTL_TCP protocol
The replay tool shall bind a TCP port, accept one client, send the captured magic packet, then stream IQ data chunks.
- **Acceptance:** An RTL_TCP client (e.g., `rtl_tcp` driver or SDR software) connecting to the replay port receives a valid magic packet followed by IQ data.

### REQ-011: Replay supports configurable speed
The replay tool shall accept a `--speed` argument (float, default 1.0). `--speed 1.0` reproduces real-time timing. `--speed 0` sends data as fast as possible without inter-chunk delays. Other values scale the inter-chunk delay accordingly.
- **Acceptance:** A 10-second capture replayed with `--speed 2.0` completes in approximately 5 seconds.

### REQ-012: Replay supports loop mode
The replay tool shall accept a `--loop` flag. When set, replay restarts from the first chunk after exhausting the file.
- **Acceptance:** `--loop --speed 0` continues sending data indefinitely with no gap between iterations.

### REQ-013: Replay logs client commands
The replay tool shall read 5-byte commands from the connected client and log each command's type and payload.
- **Acceptance:** Connecting a client that sends `0x01 0x00 0x00 0x00 0x00` produces a log line containing "SET_FREQUENCY" or the command hex.

### REQ-014: Replay exits cleanly on client disconnect
When the client disconnects, the replay tool shall log the event and shut down without panicking or leaking resources.
- **Acceptance:** Closing the client connection produces a clean exit message and zero error code.

### REQ-015: Replay streams from disk
The replay tool shall read chunks sequentially from the file on disk, not preload the entire file into memory.
- **Acceptance:** A multi-gigabyte capture file replays without consuming significant RAM.

---

## Non-Functional Requirements

### NFR-001: Captures survive disk latency
With `--buffer-mem` at the default of 64 MB, a capture of any duration shall not lose data due to disk write latency.
- **Acceptance:** In a benchmark with `fsync` after each flush, no data is lost and the output file is valid.

### NFR-002: Replay overhead is negligible
Replay at `--speed 0` shall sustain throughput comparable to or greater than the original capture rate.
- **Acceptance:** A capture written at 512 KB reads replayed at `--speed 0` saturates a gigabit link or meets the original wire speed.

### NFR-003: Graceful error handling
Both tools shall produce a human-readable error message for invalid arguments, connection failures, corrupted capture files, and other error conditions.
- **Acceptance:** `rtltcp-replay nonexistent.bin` prints a clear error instead of panicking.

---

## Out of Scope

- **Encrypted capture/replay**: Capturing from or replaying over an encrypted RTL_TCP chain is not supported in this version. Capture connects in plain mode only.
- **Multi-client replay**: Replay serves exactly one client per invocation. No slave fan-out.
- **Capture filtering**: No option to filter or transform captured data (e.g., decimate, convert format).
- **Remote replay server**: Replay binds locally only.
- **Live streaming**: Replay plays from a static file, not from a live pipe or network source.
- **Compression**: Capture files are uncompressed raw chunks.
- **Multiple files**: Replay reads a single file; no concatenation or multi-file support.
