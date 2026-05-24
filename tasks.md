# Tasks: RTL_TCP Capture & Replay Tools

Version: 1.0
Date: 2026-05-22
Depends on: design.md v1.0

---

## Phase 1: Infrastructure

**Goal:** Set up binary targets and the shared capture module.

- [ ] **TASK-001** [REQ-008] Add `[[bin]]` entries for `rtltcp-capture` and `rtltcp-replay` to `Cargo.toml`.
  - **Verify:** `cargo build` produces both binaries.

- [ ] **TASK-002** [REQ-008] Add `pub mod capture;` to `src/lib.rs`.
  - **Verify:** `cargo build` succeeds with no unused-module warnings.

- [ ] **TASK-003** [REQ-008] Implement `src/capture/mod.rs` with `CaptureHeader`, `CaptureChunk`, `write_header`, `read_header`, `write_chunk`, `read_chunk`.
  - **Verify:** `cargo build` succeeds; module is reachable from both binary targets.

---

## Phase 2: Capture Binary

**Goal:** Build and verify `rtltcp-capture`.

- [ ] **TASK-004** [REQ-001, REQ-002] Implement `src/bin/capture.rs` — parse CLI args, connect to server, read magic packet, write capture file header.
  - **Verify:** Running `rtltcp-capture --duration 1 test.bin` produces a file with valid RTLX header and correct magic_payload bytes.

- [ ] **TASK-005** [REQ-003, REQ-004] Implement capture read loop — read from socket, create timestamped chunks, check elapsed time.
  - **Verify:** A 2-second capture produces a file with multiple chunks having reasonable timestamps.

- [ ] **TASK-006** [REQ-005, REQ-006] Implement memory buffer with flush-on-threshold and flush-on-Ctrl-C. Add `--buffer-mem` option.
  - **Verify:** `^C` during capture exits cleanly with a valid file. `--buffer-mem 4096` forces frequent flushes visible in strace.

- [ ] **TASK-007** [REQ-007] Print capture stats (total bytes, elapsed time, file size) on exit.
  - **Verify:** Output contains all three metrics with non-zero values.

---

## Phase 3: Replay Binary

**Goal:** Build and verify `rtltcp-replay`.

- [ ] **TASK-008** [REQ-009, REQ-010] Implement `src/bin/replay.rs` — parse CLI args, open capture file, bind TCP, accept client, send magic packet from header.
  - **Verify:** An RTL_TCP client (or `nc`) connecting to the replay port receives the correct magic packet bytes.

- [ ] **TASK-009** [REQ-011] Implement chunk streaming with speed control — read chunk from disk, compute inter-chunk delay from `--speed`, write data to client.
  - **Verify:** `--speed 1.0` reproduces real-time duration. `--speed 0` completes near-instantly.

- [ ] **TASK-010** [REQ-012] Implement `--loop` mode — seek to first chunk on EOF and restart.
  - **Verify:** `--loop --speed 0` continues sending data indefinitely without gaps.

- [ ] **TASK-011** [REQ-013] Implement command reader thread that logs client 5-byte commands with human-readable type names.
  - **Verify:** Connecting and sending `0x01 0x00 0x11 0x22 0x33` produces a log line containing `SET_FREQUENCY` or the command hex.

- [ ] **TASK-012** [REQ-014] Implement clean shutdown on client disconnect — signal command reader, join threads, exit with code 0.
  - **Verify:** Closing the client connection results in clean exit with zero exit code.

- [ ] **TASK-013** [REQ-015] Ensure replay streams from disk (no preload). Open file once, read chunks one at a time.
  - **Verify:** `strace -e read,write replay` shows sequential `read()` calls, not one large read.

---

## Phase 4: Tests

**Goal:** Ensure correctness of the capture format and both binaries.

- [ ] **TASK-014** [REQ-008] Write unit test: round-trip header + chunks through `CaptureHeader`/`CaptureChunk` serialization to a `Vec<u8>` cursor.
  - **Verify:** `cargo test --lib capture` passes.

- [ ] **TASK-015** [REQ-009] Write integration test: generate a synthetic capture file, start replay on a port, connect a client, verify magic packet + chunk data.
  - **Verify:** `cargo test --test capture_test` passes.

- [ ] **TASK-016** [REQ-013] Write integration test: start replay, connect client, send a command, verify log output contains the command type.
  - **Verify:** `cargo test --test capture_test test_command_logging` passes.

- [ ] **TASK-017** [NFR-001] Write unit test: verify buffer flush produces contiguous output indistinguishable from unbuffered writing.
  - **Verify:** Writing chunks with flushes at arbitrary boundaries produces the same bytes as writing all chunks without flushes.

---

## Phase 5: Validation

**Goal:** End-to-end verification against real servers and clients.

- [ ] **TASK-018** [NFR-003] Test error paths: missing file, no server at host:port, corrupted capture file, invalid args.
  - **Verify:** Each error produces a clear human-readable message, not a panic.

- [ ] **TASK-019** [REQ-001..REQ-015] Manual end-to-end test: start rtltcp2026 in serve mode with slave port, capture from slave port, replay captured file, connect a third client to replay.
  - **Verify:** The third client receives valid data identical to the original serve mode stream (structure-wise; exact bytes may differ due to timing).

---

## Summary

| Phase | Tasks | Dependencies |
|---|---|---|
| 1: Infrastructure | TASK-001..TASK-003 | None |
| 2: Capture binary | TASK-004..TASK-007 | Phase 1 |
| 3: Replay binary | TASK-008..TASK-013 | Phase 1 |
| 4: Tests | TASK-014..TASK-017 | Phase 2, 3 |
| 5: Validation | TASK-018..TASK-019 | Phase 4 |
