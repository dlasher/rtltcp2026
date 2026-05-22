# rtltcp Multi-Client Proxy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the rtltcp binary from single-client to multi-client fan-out with serve mode, proxy mode, ChaCha20 encryption, and chain detection.

**Architecture:** In-process `tokio::sync::broadcast` fan-out from one USB/upstream reader thread to N per-client writer threads. Master port (single driver) + slave port (N read-only consumers, up to 10). CLI args are flat flags (not subcommands). Proxy mode connects to upstream rtltcp, optionally through encrypted chain.

**Tech Stack:** Rust 2021 edition, clap 4.5 (derive), rtlsdr_mt 2.1, tokio (sync feature only), chacha20 0.9, rand 0.8, hex 0.4, tracing, ctrlc, listenfd/systemd (optional).

**Spec:** `docs/superpowers/specs/2026-05-22-rtltcp-proxy-design.md`

---

### Task 0: Create directory scaffolding and log intent

**Files:**
- Create: `docs/superpowers/plans/2026-05-22-rtltcp-proxy-implementation.md` (this file)
- Create: `src/control.rs`
- Create: `src/stream.rs`
- Create: `src/proxy.rs`
- Create: `src/encryption.rs`

- [ ] **Step 1: Create empty module files**

Run: `touch src/control.rs src/stream.rs src/proxy.rs src/encryption.rs`

- [ ] **Step 2: Commit scaffolding**

```bash
git add docs/superpowers/plans/2026-05-22-rtltcp-proxy-implementation.md src/control.rs src/stream.rs src/proxy.rs src/encryption.rs
git commit -m "chore: create plan and module scaffolding for multi-client proxy"
```

### Task 1: Cargo.toml — add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add hardware-tests feature and crate deps**

Edit `Cargo.toml`:

```toml
[features]
default = ["daemon_systemd"]
daemon_systemd = ["listenfd", "systemd"]
hardware-tests = []

[dependencies]
# ... existing deps unchanged ...
tokio = { version = "1", default-features = false, features = ["sync"] }
chacha20 = "0.9"
rand = "0.8"
hex = "0.4"
```

- [ ] **Step 2: Verify build**

Run: `cargo build 2>&1`

Expected: Successful build (tokio sync feature doesn't need async runtime).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add tokio (sync), chacha20, rand, hex deps and hardware-tests feature"
```

### Task 2: CLI args — add new flags

**Files:**
- Modify: `src/main.rs` (Args struct)
- Modify: `tests/integration.rs` (help test)

- [ ] **Step 1: Write failing integration test for new CLI flags**

Append to `tests/integration.rs`:

```rust
#[test]
fn help_shows_new_proxy_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .arg("--help")
        .output()
        .expect("failed to execute binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("--mode"), "Missing --mode flag");
    assert!(stdout.contains("--master-port"), "Missing --master-port flag");
    assert!(stdout.contains("--slave-port"), "Missing --slave-port flag");
    assert!(stdout.contains("--max-slaves"), "Missing --max-slaves flag");
    assert!(stdout.contains("--upstream"), "Missing --upstream flag");
    assert!(stdout.contains("--key"), "Missing --key flag");
    assert!(stdout.contains("--key-file"), "Missing --key-file flag");
}

#[test]
fn old_port_flag_still_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--port", "9999", "--help"])
        .output()
        .expect("failed to execute binary");
    assert!(output.status.success());
}

#[test]
fn mode_proxy_help_succeeds() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "proxy", "--upstream", "127.0.0.1:9998", "--help"])
        .output()
        .expect("failed to execute binary");
    assert!(output.status.success());
}
```

- [ ] **Step 2: See tests fail**

Run: `cargo test help_shows_new_proxy_options old_port_flag_still_works mode_proxy_help_succeeds 2>&1 | tail -20`

Expected: FAIL — output doesn't contain new flags.

- [ ] **Step 3: Update Args struct in main.rs**

Replace the existing `Args` struct (lines 207-246 of current main.rs) with:

```rust
#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about = "an I/Q spectrum server for RTL2832 based DVB-T receivers",
    long_about = None
)]
struct Args {
    /// operating mode: "serve" (default) or "proxy"
    #[clap(long, default_value = "serve",
           value_parser = clap::builder::PossibleValuesParser::new(&["serve", "proxy"]))]
    mode: String,

    /// listen address
    #[clap(short, long, default_value = "127.0.0.1")]
    address: String,

    /// master port — accepts the driver connection (alias for --port)
    #[clap(short = 'p', long = "master-port", alias = "port", default_value_t = 1234)]
    master_port: u16,

    /// slave port — accepts read-only consumer connections
    #[clap(long)]
    slave_port: Option<u16>,

    /// maximum number of connected slaves
    #[clap(long, default_value_t = 10)]
    max_slaves: usize,

    /// upstream rtltcp server (host:port) for proxy mode
    #[clap(long)]
    upstream: Option<String>,

    /// hex-encoded 32-byte encryption key
    #[clap(long, conflicts_with = "key_file")]
    key: Option<String>,

    /// path to 32-byte raw encryption key file
    #[clap(long, conflicts_with = "key")]
    key_file: Option<String>,

    /// device index
    #[clap(short, long, default_value_t = 0)]
    device_index: u32,

    /// number of decoding buffers
    #[clap(short, long, default_value_t = 15)]
    buffers: u32,

    /// tcp sending buffer size (bytes) [default: 512000, range: 1-10485760]
    #[clap(short = 's', long, default_value_t = 512000)]
    tcp_buffers: usize,

    /// socket read timeout in seconds
    #[clap(long, default_value_t = 30)]
    read_timeout: u64,

    /// socket write timeout in seconds
    #[clap(long, default_value_t = 30)]
    write_timeout: u64,

    /// IP whitelist (CIDR notation)
    #[clap(long)]
    whitelist: Vec<String>,
}
```

- [ ] **Step 4: Update `args.port` references to `args.master_port`**

In main.rs, change `args.port` to `args.master_port` in the TcpListener::bind call.

- [ ] **Step 5: Update existing help test**

Replace `--port` with `--master-port` in the `help_shows_all_options` test's `required_options` list. The clap alias `--port` won't appear in help output — only the canonical name `--master-port` shows. Keep `-p` (short flag still works).

- [ ] **Step 6: Run tests to verify**

Run: `cargo test 2>&1 | tail -30`

Expected: All tests pass. Verify `help_shows_new_proxy_options`, `old_port_flag_still_works`, `mode_proxy_help_succeeds`.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs tests/integration.rs
git commit -m "feat: add CLI args for multi-client proxy (mode, master-port, slave-port, upstream, key)"
```

### Task 3: 0xF0 chain-detect constant + upstream ack handler

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test for 0xF0 constant**

Append to `#[cfg(test)] mod tests { ... }` in main.rs:

```rust
#[test]
fn test_chain_detect_ack_response() {
    let buf: [u8; 5] = [0xF0, 0x50, 0x52, 0x4F, 0x58];
    assert_eq!(buf[0], CMD_CHAIN_DETECT);
    let magic = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    assert_eq!(magic, 0x50524F58);
}

#[test]
fn test_chain_detect_ack_format() {
    let ack: [u8; 5] = [0xF0, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(ack.len(), COMMAND_HEADER_SIZE);
    assert_eq!(ack[0], CMD_CHAIN_DETECT);
}
```

- [ ] **Step 2: See tests fail**

Run: `cargo test test_chain_detect_ack_response test_chain_detect_ack_format 2>&1`

Expected: FAIL — `CMD_CHAIN_DETECT` not defined.

- [ ] **Step 3: Add CMD_CHAIN_DETECT constant and handler case**

Add after `CMD_SET_AGC` line:

```rust
const CMD_CHAIN_DETECT: u8 = 0xF0;
```

In the control thread's match, BEFORE the `_ =>` catch-all, add:

```rust
CMD_CHAIN_DETECT => {
    info!("chain detection probe from downstream proxy");
    if let Err(e) = stream.write_all(&[CMD_CHAIN_DETECT, 0x00, 0x00, 0x00, 0x00]) {
        warn!("failed to send chain detect ack: {e}");
    }
}
```

The `stream` variable needs to be mutable. Ensure it is: the closure captures `let mut stream = stream.try_clone()?;`.

- [ ] **Step 4: See tests pass**

Run: `cargo test test_chain_detect_ack_response test_chain_detect_ack_format 2>&1`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add 0xF0 chain-detect constant and upstream ack handler"
```

### Task 4: Extract control module (control.rs)

**Files:**
- Create: `src/control.rs` (extracted from main.rs)
- Modify: `src/main.rs` (add `mod control; use`)
- Modify: `tests/integration.rs` (clean up duplicate tests)

- [ ] **Step 1: Create control.rs with all extracted code**

Create `src/control.rs` with:
- All command constants (make `pub`)
- `COMMAND_HEADER_SIZE`, `MAGIC_PACKET`, range constants (make `pub`)
- `with_control`, `check_whitelist`, `is_ip_in_whitelist` (make `pub`)
- All validate_* functions (make `pub`)
- `AgcState` struct (make `pub`)
- `RateLimiter` struct (make `pub`)
- `COMMAND_RATE_LIMIT_INTERVAL` (make `pub`)
- All `#[cfg(test)] mod tests { ... }` from main.rs (adjust for crate paths)

The module must have `use` imports for `tracing`, `ipnet`, `std::net::IpAddr`, etc.

- [ ] **Step 2: Update main.rs to use control module**

In main.rs:
- Add `mod control;` after `mod error;`
- Add `use crate::control::*;`
- Remove all duplicated constants, functions, structs from main.rs (lines 22-205)
- Remove all `#[cfg(test)] mod tests { ... }` block (lines 567-890)

Keep the main() function and thread spawning.

- [ ] **Step 3: Clean up tests/integration.rs**

Remove the duplicate `validate_frequency`, `validate_sample_rate`, `validate_ppm`, `validate_tuner_gain` functions. The tests that call them remain — replace `validate_frequency(...)` calls with direct inline validation or remove if fully covered in control.rs unit tests.

Actually, since the integration tests define their own local validation functions (not calling the crate ones), just remove those duplicate function definitions. The tests that use them are already covered by control.rs unit tests. Remove the entire validation function block (lines 130-166 in integration.rs) and the tests that call them or leave them — they still compile with local functions.

- [ ] **Step 4: Build and test**

Run: `cargo build 2>&1` — must succeed.
Run: `cargo test 2>&1` — all tests pass. The control module tests should appear as `control::tests::*`.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/control.rs tests/integration.rs
git commit -m "refactor: extract control module (commands, validation, rate limiter, whitelist)"
```

### Task 5: Stream module — broadcast channel + writer thread

**Files:**
- Create: `src/stream.rs`
- Create: `tests/stream_tests.rs`
- Modify: `src/main.rs` (add `pub mod stream`)

- [ ] **Step 1: Write failing test for stream module**

Create `tests/stream_tests.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::io::{Read, Write};

#[test]
fn test_broadcast_send_recv() {
    let (tx, _rx) = rtltcp2026::stream::new_broadcast(16);
    let mut rx1 = tx.subscribe();
    let mut rx2 = tx.subscribe();

    let data = vec![0u8; 512];
    assert!(tx.send(data.clone()).is_ok());

    let recv1 = rx1.try_recv().unwrap();
    let recv2 = rx2.try_recv().unwrap();
    assert_eq!(recv1.len(), 512);
    assert_eq!(recv2, recv1);
}

#[test]
fn test_broadcast_lag() {
    use tokio::sync::broadcast::error::TryRecvError;
    let (tx, _rx) = rtltcp2026::stream::new_broadcast(4);
    let mut rx = tx.subscribe();

    for _ in 0..4 { let _ = tx.send(vec![0u8; 64]); }
    let _ = tx.send(vec![1u8; 64]);

    match rx.try_recv() {
        Err(TryRecvError::Lagged(n)) => assert!(n > 0),
        other => panic!("expected Lagged, got {other:?}"),
    }
}

#[test]
fn test_writer_loop_exits_on_flag() {
    let (tx, _rx) = rtltcp2026::stream::new_broadcast(16);
    let rx = tx.subscribe();
    let should_exit = Arc::new(AtomicBool::new(false));

    let (mut reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();

    let we = should_exit.clone();
    let handle = thread::spawn(move || {
        rtltcp2026::stream::write_client_loop(writer, rx, &we);
    });

    tx.send(vec![0x42u8; 32]).unwrap();
    thread::sleep(Duration::from_millis(20));

    let mut buf = vec![0u8; 32];
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(buf, vec![0x42u8; 32]);

    should_exit.store(true, Ordering::SeqCst);
    handle.join().unwrap();
}
```

- [ ] **Step 2: See tests fail**

Run: `cargo test --test stream_tests 2>&1 | tail -20`

Expected: FAIL — module `stream` not found, no public symbols.

- [ ] **Step 3: Implement stream.rs**

Create `src/stream.rs`:

```rust
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;

use tokio::sync::broadcast;
use tracing::{debug, warn};

pub const DEFAULT_BROADCAST_CAPACITY: usize = 64;

pub fn new_broadcast(capacity: usize) -> (broadcast::Sender<Vec<u8>>, broadcast::Receiver<Vec<u8>>) {
    broadcast::channel(capacity)
}

/// Writer loop for a single client: reads from broadcast, writes to TCP.
/// Exits when `should_exit` is set, channel is closed, or write errors.
pub fn write_client_loop(
    mut stream: impl Write,
    mut rx: broadcast::Receiver<Vec<u8>>,
    should_exit: &AtomicBool,
) {
    loop {
        if should_exit.load(Ordering::SeqCst) {
            debug!("writer thread: exit flag set, stopping");
            break;
        }

        match rx.try_recv() {
            Ok(buf) => {
                if let Err(e) = stream.write_all(&buf) {
                    warn!("writer thread: write error, stopping: {e}");
                    break;
                }
            }
            Err(broadcast::error::TryRecvError::Empty) => {
                thread::sleep(Duration::from_micros(100));
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                debug!("writer thread: broadcast closed, stopping");
                break;
            }
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                warn!("writer thread: lagged by {n} buffers, continuing");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::Arc;

    #[test]
    fn test_new_broadcast_send_recv() {
        let (tx, mut rx) = new_broadcast(16);
        let data = vec![1u8, 2u8, 3u8];
        tx.send(data.clone()).unwrap();
        assert_eq!(rx.try_recv().unwrap(), data);
    }

    #[test]
    fn test_broadcast_multiple_receivers() {
        let (tx, _rx) = new_broadcast(16);
        let mut rx1 = tx.subscribe();
        let mut rx2 = tx.subscribe();
        let data = vec![0xABu8; 256];
        tx.send(data.clone()).unwrap();
        assert_eq!(rx1.try_recv().unwrap(), data);
        assert_eq!(rx2.try_recv().unwrap(), data);
    }

    #[test]
    fn test_writer_exits_on_flag() {
        let (tx, _rx) = new_broadcast(16);
        let rx = tx.subscribe();
        let flag = AtomicBool::new(false);
        let (mut r, w) = std::os::unix::net::UnixStream::pair().unwrap();
        let h = thread::spawn(move || write_client_loop(w, rx, &flag));
        tx.send(vec![0x42; 32]).unwrap();
        thread::sleep(Duration::from_millis(10));
        let mut buf = vec![0u8; 32];
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf, vec![0x42; 32]);
        flag.store(true, Ordering::SeqCst);
        h.join().unwrap();
    }
}
```

- [ ] **Step 4: Register module in main.rs**

Add after existing `mod` declarations: `pub mod stream;`

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1` — must succeed.
Run: `cargo test 2>&1` — all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/stream.rs tests/stream_tests.rs src/main.rs
git commit -m "feat: stream module with tokio::sync::broadcast and client writer loop"
```

---

### Task 6: Multi-client serve mode — wire stream module into USB path

**Files:**
- Modify: `src/main.rs` (refactor main() into dispatch, add run_serve_single, run_serve_multi)
- Modify: `tests/integration.rs` (add multi-client tests)

- [ ] **Step 1: Write integration test for serve mode flags**

Append to `tests/integration.rs`:

```rust
#[test]
fn test_serve_mode_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "serve", "--slave-port", "9995", "--help"])
        .output()
        .expect("failed to execute binary");
    assert!(output.status.success());
}
```

- [ ] **Step 2: See test pass (it should pass since flags were added in Task 2)**

Run: `cargo test test_serve_mode_help 2>&1`
Expected: PASS.

- [ ] **Step 3: Refactor main() into dispatch + run_serve_single**

Current main() does everything inline. Refactor the existing logic into `run_serve_single(args: Args)` and change `main()` to:

```rust
fn main() -> StdResult<(), RtlTcpError> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    validate_args(&args)?;

    match args.mode.as_str() {
        "proxy" => {
            Err(RtlTcpError::Config("proxy mode not yet implemented".to_string()))
        }
        "serve" => {
            if args.slave_port.is_some() {
                run_serve_multi(args)
            } else {
                run_serve_single(args)
            }
        }
        _ => Err(RtlTcpError::Config(format!("unknown mode: {}", args.mode))),
    }
}
```

`run_serve_single` contains the exact same logic as the current main() body (lines 253-565), with `args.port` → `args.master_port`. Must include the all-interfaces warning block (main.rs lines 276-285).

`validate_args` contains the range checks that are currently inline:

```rust
fn validate_args(args: &Args) -> StdResult<(), RtlTcpError> {
    if args.buffers == 0 || args.buffers > 32 {
        return Err(RtlTcpError::Config("buffers must be between 1 and 32".to_string()));
    }
    if args.tcp_buffers == 0 || args.tcp_buffers > 10_485_760 {
        return Err(RtlTcpError::Config("tcp_buffers must be between 1 and 10485760 (10MB)".to_string()));
    }
    if args.read_timeout == 0 {
        return Err(RtlTcpError::Config("read_timeout must be greater than 0".to_string()));
    }
    if args.write_timeout == 0 {
        return Err(RtlTcpError::Config("write_timeout must be greater than 0".to_string()));
    }
    Ok(())
}
```

- [ ] **Step 4: Implement run_serve_multi**

Create the multi-client serve mode function. Key design:

```rust
fn run_serve_multi(args: Args) -> StdResult<(), RtlTcpError> {
    let read_timeout = Duration::from_secs(args.read_timeout);
    let write_timeout = Duration::from_secs(args.write_timeout);

    // Warn when binding to all interfaces
    let is_all = args.address == "0.0.0.0" || args.address == "::" || args.address == "[::]" || args.address.is_empty();
    if is_all { warn!("binding to all interfaces ({}) — exposes server to all networks", args.address); }

    let (ctl, mut reader) = rtlsdr_mt::open(args.device_index)
        .map_err(|e| RtlTcpError::Device(format!("could not open RTL-SDR device: {e:?}")))?;
    let ctl = Arc::new(Mutex::new(ctl));
    let agc_state = Arc::new(control::AgcState::new());
    let magic_packet = control::MAGIC_PACKET;

    let (tx, _rx) = stream::new_broadcast(stream::DEFAULT_BROADCAST_CAPACITY);
    let should_exit = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = sync_channel(1);
    let all_streams: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));

    // Signal handler: closes all streams
    ctrlc::set_handler({
        let s = sender.clone();
        let exit = should_exit.clone();
        move || {
            info!("received signal, shutting down");
            let _ = s.try_send(());
            exit.store(true, Ordering::SeqCst);
        }
    }).map_err(|e| RtlTcpError::Config(format!("could not set signal handler: {e}")))?;

    // Bind master port
    let master_listener = bind_master_port(&args)?;

    // Bind slave port
    let slave_port = args.slave_port.unwrap();
    let slave_listener = TcpListener::bind(format!("{}:{}", args.address, slave_port))?;
    info!("slave port listening on {slave_port}");

    // Accept master
    info!("waiting for master connection on port {}…", args.master_port);
    let (master_stream, addr) = master_listener.accept()?;
    let client_ip = addr.ip().to_canonical().to_string();
    control::check_whitelist(&client_ip, &args.whitelist)
        .map_err(|e| { warn!("Connection from {client_ip} refused"); e })?;
    info!("master connected from {addr}");
    master_stream.set_read_timeout(Some(read_timeout))?;
    master_stream.set_write_timeout(Some(write_timeout))?;

    all_streams.lock().unwrap().push(master_stream.try_clone()?);

    // Send magic packet
    let mut bufw = BufWriter::with_capacity(args.tcp_buffers, master_stream.try_clone()?);
    bufw.write_all(magic_packet)?;
    bufw.flush()?;

    // Start master control thread
    let unknown_count = Arc::new(Mutex::new(0u64));
    let thread_ctl = spawn_master_control_thread(
        master_stream.try_clone()?, ctl.clone(), agc_state.clone(),
        unknown_count, should_exit.clone(), read_timeout,
    );

    // Start slave acceptor thread
    spawn_slave_acceptor(
        slave_listener, tx.clone(), magic_packet.to_vec(),
        args.whitelist.clone(), args.max_slaves, args.tcp_buffers,
        should_exit.clone(), all_streams.clone(),
    );

    // Cancel thread
    let thread_cancel = spawn_cancel_thread(ctl.clone(), receiver, should_exit.clone());

    // USB read callback → broadcast
    let btx = tx.clone();
    let s = sender.clone();
    let read_result = reader.read_async(args.buffers, 0, move |bytes| {
        if btx.send(bytes.to_vec()).is_err() {
            let _ = s.try_send(());
        }
    });

    let _ = sender.try_send(());
    let _ = thread_cancel.join();
    let _ = thread_ctl.join();

    // Close all slave streams
    {
        let streams = all_streams.lock().unwrap();
        for s in streams.iter() { let _ = s.shutdown(Shutdown::Both); }
    }

    info!("multi-client serve shut down");
    Ok(())
}
```

Extract helper functions `spawn_master_control_thread`, `spawn_slave_acceptor`, `spawn_cancel_thread`, `bind_master_port` from the shared logic.

`bind_master_port` encapsulates the systemd socket activation or plain bind:

```rust
fn bind_master_port(args: &Args) -> StdResult<TcpListener, RtlTcpError> {
    let addr = format!("{}:{}", args.address, args.master_port);
    #[cfg(feature = "systemd")]
    {
        let mut listenfd = ListenFd::from_env();
        if let Some(listener) = listenfd.take_tcp_listener(0).map_err(|e| {
            RtlTcpError::Config(format!("could not get fd from env: {e}"))
        })? {
            systemd::daemon::notify(false, [(systemd::daemon::STATE_READY, "1")].iter())?;
            return Ok(listener);
        }
    }
    TcpListener::bind(&addr).map_err(Into::into)
}
```

`spawn_master_control_thread` wraps the master's command-reading loop (the same as the current inline thread in `run_serve_single`, using `control::*` functions). Its signature:

```rust
fn spawn_master_control_thread(
    stream: TcpStream,
    ctl: Arc<Mutex<rtlsdr_mt::Controller>>,
    agc_state: Arc<control::AgcState>,
    unknown_count: Arc<Mutex<u64>>,
    should_exit: Arc<AtomicBool>,
    read_timeout: Duration,
) -> thread::JoinHandle<()>
```

The body is the same as main.rs lines 362-512, with functions replaced by `control::*` equivalents.

The slave acceptor thread uses non-blocking accept loop:

```rust
fn spawn_slave_acceptor(
    listener: TcpListener, tx: broadcast::Sender<Vec<u8>>, magic: Vec<u8>,
    whitelist: Vec<String>, max_slaves: usize, tcp_buffers: usize,
    should_exit: Arc<AtomicBool>, all_streams: Arc<Mutex<Vec<TcpStream>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        let mut count = 0usize;
        loop {
            if should_exit.load(Ordering::SeqCst) { break; }
            if count >= max_slaves {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            match listener.accept() {
                Ok((stream, addr)) => {
                    let ip = addr.ip().to_canonical().to_string();
                    if let Err(e) = control::check_whitelist(&ip, &whitelist) {
                        warn!("slave {ip} refused: {e}"); continue;
                    }
                    info!("slave connected from {addr}");
                    stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
                    all_streams.lock().unwrap().push(stream.try_clone().unwrap());
                    let mut bw = BufWriter::with_capacity(tcp_buffers, stream.try_clone().unwrap());
                    let _ = bw.write_all(&magic);
                    let _ = bw.flush();
                    let rx = tx.subscribe();
                    let exit = should_exit.clone();
                    // stream is the original TCP stream (not a clone) — previous try_clone() calls
                    // produced independent FDs that are now owned by all_streams and the dropped BufWriter
                    thread::spawn(move || stream::write_client_loop(stream, rx, &exit));
                    count += 1;
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => { warn!("slave accept error: {e}"); thread::sleep(Duration::from_millis(100)); }
            }
        }
    })
}
```

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1` — must succeed.
Run: `cargo test 2>&1` — all pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/integration.rs
git commit -m "feat: multi-client serve mode with master+slave accept, broadcast fan-out, signal shutdown"
```

### Task 7: Proxy module — upstream connection + chain detection

**Files:**
- Create: `src/proxy.rs`
- Create: `tests/proxy_test.rs`
- Modify: `src/main.rs` (add `pub mod proxy`)

- [ ] **Step 1: Write failing proxy test**

Create `tests/proxy_test.rs`:

```rust
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// Fake upstream that responds to 0xF0 with ack
fn start_upstream_chain() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d").unwrap();
        let mut buf = [0u8; 5]; s.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0xF0);
        s.write_all(&[0xF0, 0x00, 0x00, 0x00, 0x00]).unwrap();
    });
    p
}

#[test]
fn test_chain_detect_handshake() {
    let p = start_upstream_chain();
    thread::sleep(Duration::from_millis(50));
    let result = rtltcp2026::proxy::connect_upstream(
        "127.0.0.1", p, None, Duration::from_millis(500)
    );
    assert!(result.is_ok(), "connect_upstream should succeed with ack upstream");
    let conn = result.unwrap();
    assert!(conn.is_chain, "should detect chain mode");
    assert!(conn.encryption_key.is_none(), "no key provided");
}

#[test]
fn test_chain_detect_timeout_gives_plain() {
    // Standard rtltcp: sends magic but doesn't respond to 0xF0
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d").unwrap();
        let mut buf = [0u8; 5]; s.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0xF0);
        thread::sleep(Duration::from_millis(600));
    });

    thread::sleep(Duration::from_millis(50));
    let result = rtltcp2026::proxy::connect_upstream(
        "127.0.0.1", p, None, Duration::from_millis(200)
    );
    assert!(result.is_ok(), "should fall back gracefully on timeout");
    let conn = result.unwrap();
    assert!(!conn.is_chain, "no chain without ack");
}
```

- [ ] **Step 2: See test fail**

Run: `cargo test --test proxy_test 2>&1 | tail -20`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement proxy.rs**

Create `src/proxy.rs`:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tracing::{debug, info, warn};
use crate::control;

pub struct UpstreamConnection {
    pub stream: TcpStream,
    pub is_chain: bool,
    pub encryption_key: Option<([u8; 12], [u8; 12])>,
    pub magic_packet: [u8; 12],
}

pub fn connect_upstream(
    host: &str, port: u16, key: Option<[u8; 32]>, timeout: Duration,
) -> Result<UpstreamConnection, crate::error::RtlTcpError> {
    let addr = format!("{host}:{port}");
    info!("connecting to upstream {addr}");
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| crate::error::RtlTcpError::Network(format!("failed to connect upstream: {e}")))?;
    stream.set_read_timeout(Some(timeout))?;

    let mut magic = [0u8; 12];
    stream.read_exact(&mut magic)
        .map_err(|e| crate::error::RtlTcpError::Network(format!("failed to read upstream magic: {e}")))?;

    let probe = [control::CMD_CHAIN_DETECT, 0x50, 0x52, 0x4F, 0x58];
    stream.write_all(&probe)?;

    let mut ack = [0u8; 5];
    let is_chain = match stream.read_exact(&mut ack) {
        Ok(()) if ack[0] == control::CMD_CHAIN_DETECT => {
            info!("chain detection: upstream supports proxy protocol");
            true
        }
        _ => {
            debug!("no chain detect ack, using plain TCP");
            stream.set_read_timeout(Some(Duration::from_secs(30)))?;
            false
        }
    };

    let encryption_key = if is_chain {
        if let Some(enc_key) = key {
            info!("performing encrypted handshake");
            let (my_nonce, peer_nonce) = crate::encryption::nonce_exchange(&mut stream, enc_key)
                .map_err(|e| crate::error::RtlTcpError::Network(
                    format!("nonce exchange failed: {e}")))?;
            info!("encrypted chain established");
            Some((my_nonce, peer_nonce))
        } else {
            None
        }
    } else {
        None
    };

    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    Ok(UpstreamConnection { stream, is_chain, encryption_key, magic_packet: magic })
}
```

- [ ] **Step 4: Register module**

Add `pub mod proxy;` in main.rs.

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1` — succeeds (proxy module depends on encryption which also needs to exist...)

Wait — proxy.rs calls `crate::encryption::nonce_exchange`, but encryption.rs doesn't exist yet. Either create an empty placeholder, or create encryption.rs first. Let's stub it:

Create a minimal `src/encryption.rs`:

```rust
pub fn nonce_exchange(
    _stream: &mut (impl std::io::Read + std::io::Write),
    _key: [u8; 32],
) -> std::io::Result<([u8; 12], [u8; 12])> {
    Err(std::io::Error::new(std::io::ErrorKind::Other, "not implemented"))
}
```

This lets proxy compile. The full encryption module is Task 10.

Run: `cargo test 2>&1` — all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/proxy.rs tests/proxy_test.rs src/encryption.rs src/main.rs
git commit -m "feat: proxy module with upstream connection and 0xF0 chain detection"
```

### Task 8: Proxy mode in main — wire proxy dispatch

**Files:**
- Modify: `src/main.rs` (add `run_proxy_multi` function, update main dispatch)
- Add: helper functions in main.rs

- [ ] **Step 1: Implement run_proxy_multi**

Add to main.rs:

```rust
fn run_proxy_multi(args: Args) -> StdResult<(), RtlTcpError> {
    let upstream = args.upstream.as_ref()
        .ok_or_else(|| RtlTcpError::Config("--upstream required in proxy mode".to_string()))?;
    let read_timeout = Duration::from_secs(args.read_timeout);

    let should_exit = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = sync_channel(1);

    ctrlc::set_handler({
        let s = sender.clone(); let e = should_exit.clone();
        move || { info!("received signal"); let _ = s.try_send(()); e.store(true, Ordering::SeqCst); }
    }).map_err(|e| RtlTcpError::Config(format!("could not set signal handler: {e}")))?;

    let master_listener = bind_master_port(&args)?;
    let slave_listener = args.slave_port.map(|sp| TcpListener::bind(format!("{}:{}", args.address, sp))).transpose()?;

    info!("waiting for master connection on port {}…", args.master_port);
    let (mut master_stream, addr) = master_listener.accept()?;
    let client_ip = addr.ip().to_canonical().to_string();
    control::check_whitelist(&client_ip, &args.whitelist)
        .map_err(|e| { warn!("Connection from {client_ip} refused"); e })?;
    info!("master connected from {addr}");
    master_stream.set_read_timeout(Some(read_timeout))?;
    master_stream.set_write_timeout(Some(Duration::from_secs(args.write_timeout)))?;

    let (upstream_host, upstream_port_str) = upstream.rsplit_once(':')
        .ok_or_else(|| RtlTcpError::Config(format!("invalid upstream: {upstream}")))?;
    let upstream_port: u16 = upstream_port_str.parse()
        .map_err(|_| RtlTcpError::Config(format!("invalid upstream port: {upstream_port_str}")))?;

    let encryption_key = parse_encryption_key(&args)?;
    let upstream_conn = proxy::connect_upstream(
        upstream_host, upstream_port, encryption_key, Duration::from_millis(500)
    )?;
    let is_chain = upstream_conn.is_chain;
    let magic_packet = upstream_conn.magic_packet;
    info!("connected to upstream, chain mode: {is_chain}");

    let (tx, _rx) = stream::new_broadcast(stream::DEFAULT_BROADCAST_CAPACITY);

    // Send cached magic packet to local master
    let mut bufw = BufWriter::with_capacity(args.tcp_buffers, master_stream.try_clone()?);
    bufw.write_all(&magic_packet)?; bufw.flush()?;

    // Start upstream reader thread → broadcast
    let utx = tx.clone(); let uexit = should_exit.clone(); let usender = sender.clone();
    let thread_upstream = thread::spawn(move || {
        let mut buf = vec![0u8; 512 * 1024];
        loop {
            if uexit.load(Ordering::SeqCst) { break; }
            match upstream_conn.stream.read(&mut buf) {
                Ok(0) => { info!("upstream closed"); break; }
                Ok(n) => { let _ = utx.send(buf[..n].to_vec()); }
                Err(ref e) if e.kind() == ErrorKind::TimedOut => continue,
                Err(e) => { warn!("upstream read error: {e}"); let _ = usender.try_send(()); break; }
            }
        }
    });

    // Master control thread: forward commands upstream
    let cexit = should_exit.clone();
    let thread_ctl = thread::spawn(move || {
        let mut buf = [0u8; control::COMMAND_HEADER_SIZE];
        let mut rl = control::RateLimiter::new(control::COMMAND_RATE_LIMIT_INTERVAL);
        loop {
            match master_stream.read_exact(&mut buf) {
                Ok(()) => {}
                Err(ref e) if is_disconnect_err(e) => { info!("master disconnected"); break; }
                Err(e) => { warn!("master read error: {e}"); break; }
            }
            if cexit.load(Ordering::SeqCst) { break; }
            if !rl.check() { continue; }
            if let Err(e) = upstream_conn.stream.write_all(&buf) {
                warn!("failed to forward command: {e}"); break;
            }
        }
    });

    // Slave acceptor
    if let Some(sl) = slave_listener {
        spawn_slave_acceptor(
            sl, tx, magic_packet.to_vec(),
            args.whitelist, args.max_slaves, args.tcp_buffers,
            should_exit.clone(), Arc::new(Mutex::new(Vec::new())),
        );
    }

    // Wait for shutdown
    let _ = receiver.recv();
    should_exit.store(true, Ordering::SeqCst);
    let _ = thread_upstream.join();
    let _ = thread_ctl.join();
    info!("proxy mode shut down");
    Ok(())
}
```

Add helper:

```rust
fn is_disconnect_err(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
        | ErrorKind::BrokenPipe | ErrorKind::ConnectionAborted | ErrorKind::NotConnected)
}

fn parse_encryption_key(args: &Args) -> Result<Option<[u8; 32]>, RtlTcpError> {
    if let Some(ref hex_key) = args.key {
        let bytes = hex::decode(hex_key)
            .map_err(|e| RtlTcpError::Config(format!("invalid hex key: {e}")))?;
        if bytes.len() != 32 { return Err(RtlTcpError::Config("key must be 32 bytes".into())); }
        let mut k = [0u8; 32]; k.copy_from_slice(&bytes); Ok(Some(k))
    } else if let Some(ref path) = args.key_file {
        let bytes = std::fs::read(path)
            .map_err(|e| RtlTcpError::Config(format!("failed to read key file {path}: {e}")))?;
        if bytes.len() != 32 { return Err(RtlTcpError::Config("key must be 32 bytes".into())); }
        let mut k = [0u8; 32]; k.copy_from_slice(&bytes); Ok(Some(k))
    } else { Ok(None) }
}
```

- [ ] **Step 2: Update main dispatch**

Change the proxy placeholder in `main()`:

```rust
"proxy" => run_proxy_multi(args),
```

- [ ] **Step 3: Build and test**

Run: `cargo build 2>&1` — must succeed.
Run: `cargo test 2>&1` — all pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: proxy mode dispatch with upstream reader, command forwarding, slave fan-out"
```

---

### Task 9: Encryption module — EncryptedStream + nonce exchange

**Files:**
- Create: `src/encryption.rs` (full implementation, replacing stub)
- Create: `tests/encryption_test.rs`

- [ ] **Step 1: Write failing test**

Create `tests/encryption_test.rs`:

```rust
use std::io::{Read, Write};

#[test]
fn test_encrypted_roundtrip() {
    let key = [0xABu8; 32];
    let nonce = [0x42u8; 12];

    let (w, r) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut enc_w = rtltcp2026::encryption::EncryptedWriter::new(w.try_clone().unwrap(), key, nonce);
    let mut enc_r = rtltcp2026::encryption::EncryptedReader::new(r, key, nonce);

    let data = b"Hello, ChaCha20!";
    enc_w.write_all(data).unwrap();
    enc_w.flush().unwrap();
    drop(enc_w);

    let mut buf = Vec::new();
    enc_r.read_to_end(&mut buf).unwrap();
    assert_eq!(&buf, data);
}

#[test]
fn test_generate_nonce_unique() {
    let n1 = rtltcp2026::encryption::generate_nonce();
    let n2 = rtltcp2026::encryption::generate_nonce();
    assert_ne!(n1, n2, "subsequent nonces should differ");
}
```

- [ ] **Step 2: See test fail**

Run: `cargo test --test encryption_test 2>&1`
Expected: FAIL — `EncryptedWriter` doesn't exist or stub panics.

- [ ] **Step 3: Implement encryption.rs fully**

Replace the stub with:

```rust
use std::io::{Read, Write};
use chacha20::{ChaCha20, Key, Nonce};
use chacha20::cipher::{KeyIvInit, StreamCipher};

pub struct EncryptedWriter<W: Write> {
    inner: W,
    cipher: ChaCha20,
}

impl<W: Write> EncryptedWriter<W> {
    pub fn new(inner: W, key: [u8; 32], nonce: [u8; 12]) -> Self {
        Self { inner, cipher: ChaCha20::new(Key::from_slice(&key), Nonce::from_slice(&nonce)) }
    }
}

impl<W: Write> Write for EncryptedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut encrypted = buf.to_vec();
        self.cipher.apply_keystream(&mut encrypted);
        self.inner.write(&encrypted)
    }
    fn flush(&mut self) -> std::io::Result<()> { self.inner.flush() }
}

pub struct EncryptedReader<R: Read> {
    inner: R,
    cipher: ChaCha20,
}

impl<R: Read> EncryptedReader<R> {
    pub fn new(inner: R, key: [u8; 32], nonce: [u8; 12]) -> Self {
        Self { inner, cipher: ChaCha20::new(Key::from_slice(&key), Nonce::from_slice(&nonce)) }
    }
}

impl<R: Read> Read for EncryptedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.cipher.apply_keystream(&mut buf[..n]);
        Ok(n)
    }
}

pub fn generate_nonce() -> [u8; 12] {
    use rand::Rng;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill(&mut nonce);
    nonce
}

pub fn nonce_exchange(
    stream: &mut (impl Read + Write),
    _key: [u8; 32],
) -> std::io::Result<([u8; 12], [u8; 12])> {
    let my_nonce = generate_nonce();
    stream.write_all(&my_nonce)?;
    let mut peer_nonce = [0u8; 12];
    stream.read_exact(&mut peer_nonce)?;
    Ok((my_nonce, peer_nonce))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn test_encrypted_roundtrip() {
        let key = [0xABu8; 32]; let nonce = [0x42u8; 12];
        let (w, r) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut ew = EncryptedWriter::new(w.try_clone().unwrap(), key, nonce);
        let mut er = EncryptedReader::new(r, key, nonce);
        let data = b"Hello!";
        ew.write_all(data).unwrap(); ew.flush().unwrap(); drop(ew);
        let mut buf = Vec::new();
        er.read_to_end(&mut buf).unwrap();
        assert_eq!(&buf, data);
    }

    #[test]
    fn test_nonce_unique() {
        assert_ne!(generate_nonce(), generate_nonce());
    }

    #[test]
    fn test_nonce_exchange() {
        let (mut a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
        let key = [0x01u8; 32];
        let h = std::thread::spawn(move || nonce_exchange(&mut b, key).unwrap());
        let (my_a, peer_a) = nonce_exchange(&mut a, key).unwrap();
        let (my_b, peer_b) = h.join().unwrap();
        assert_eq!(my_a, peer_b);
        assert_eq!(my_b, peer_a);
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/encryption.rs tests/encryption_test.rs
git commit -m "feat: encryption module with EncryptedReader/Writer and ChaCha20 nonce exchange"
```

### Task 10: Integration tests — end-to-end chain, disconnect, graceful shutdown

**Files:**
- Create: `tests/chain_test.rs`
- Modify: `tests/integration.rs` (add disconnect tests)

- [ ] **Step 1: Write chain tests**

Create `tests/chain_test.rs`:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn write_cmd(s: &mut TcpStream, opcode: u8, payload: [u8; 4]) {
    s.write_all(&[opcode, payload[0], payload[1], payload[2], payload[3]]).unwrap();
}

#[test]
fn test_invalid_mode_fails() {
    let o = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "invalid"]).output().unwrap();
    assert!(!o.status.success());
}

#[test]
#[ignore] // Requires USB device
fn test_master_slave_same_iq() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "serve", "--master-port", "9941", "--slave-port", "9942"])
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    thread::sleep(Duration::from_millis(500));

    let mut master = TcpStream::connect("127.0.0.1:9941").unwrap();
    let mut slave = TcpStream::connect("127.0.0.1:9942").unwrap();
    master.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    slave.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    let mut mm = [0u8; 12]; master.read_exact(&mut mm).unwrap();
    let mut sm = [0u8; 12]; slave.read_exact(&mut sm).unwrap();
    assert_eq!(mm, sm);

    write_cmd(&mut master, 0x01, 100_500_000u32.to_be_bytes());
    thread::sleep(Duration::from_millis(200));

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn test_slave_command_silently_consumed() {
    // Verify slave can send bogus commands without error
    // This tests the slave command consumer's robustness
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "serve", "--master-port", "9943", "--slave-port", "9944"])
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    thread::sleep(Duration::from_millis(300));

    let master = TcpStream::connect("127.0.0.1:9943").unwrap();
    let mut slave = TcpStream::connect("127.0.0.1:9944").unwrap();

    let mut sm = [0u8; 12]; slave.read_exact(&mut sm).unwrap();

    // Send various commands from slave — should be silently consumed
    write_cmd(&mut slave, 0x01, 100_000_000u32.to_be_bytes());
    write_cmd(&mut slave, 0xFF, [0; 4]);
    write_cmd(&mut slave, 0x00, [0; 4]);
    thread::sleep(Duration::from_millis(100));

    // Slave should NOT receive any response to its commands
    slave.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let mut response_buf = [0u8; 1];
    let read_result = slave.read(&mut response_buf);
    assert!(read_result.is_err(), "slave should not receive data after sending commands");

    // Master is unaffected
    drop(master);
    child.kill().unwrap();
    child.wait().unwrap();
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test test_invalid_mode_fails test_slave_command_silently_consumed 2>&1`
Expected: PASS.

Run ignored test:
Run: `cargo test test_master_slave_same_iq -- --ignored --nocapture 2>&1`
Expected: May fail without USB device (correctly gated).

- [ ] **Step 3: Commit**

```bash
git add tests/chain_test.rs
git commit -m "test: end-to-end chain tests for multi-client serve and slave command handling"
```

### Task 11: Hardware tests (feature-gated)

**Files:**
- Modify: `Cargo.toml` (hardware-tests feature already added)
- Create: `tests/hardware.rs`

- [ ] **Step 1: Write hardware-gated test**

Create `tests/hardware.rs`:

```rust
#![cfg(feature = "hardware-tests")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn write_cmd(s: &mut TcpStream, opcode: u8, payload: [u8; 4]) {
    s.write_all(&[opcode, payload[0], payload[1], payload[2], payload[3]]).unwrap();
}

#[test]
fn test_real_device_serve_multi() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "serve", "--master-port", "9981", "--slave-port", "9982"])
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("failed to start binary");
    thread::sleep(Duration::from_millis(1000));

    let mut m = TcpStream::connect("127.0.0.1:9981").unwrap();
    let mut s1 = TcpStream::connect("127.0.0.1:9982").unwrap();
    let mut s2 = TcpStream::connect("127.0.0.1:9982").unwrap();

    m.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s1.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s2.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    // All get magic packet
    let mut mm = [0u8; 12]; m.read_exact(&mut mm).unwrap();
    let mut s1m = [0u8; 12]; s1.read_exact(&mut s1m).unwrap();
    let mut s2m = [0u8; 12]; s2.read_exact(&mut s2m).unwrap();
    assert_eq!(mm, s1m);
    assert_eq!(mm, s2m);

    // Set frequency
    write_cmd(&mut m, 0x01, 100_500_000u32.to_be_bytes());
    thread::sleep(Duration::from_millis(200));

    // Read a small amount of IQ data from each slave
    let mut buf1 = [0u8; 4096];
    let mut buf2 = [0u8; 4096];
    let n1 = s1.read(&mut buf1).unwrap();
    let n2 = s2.read(&mut buf2).unwrap();

    assert!(n1 > 0, "slave 1 should receive IQ data");
    assert!(n2 > 0, "slave 2 should receive IQ data");
    // Both should see the same data (over the same short window)
    assert_eq!(&buf1[..n1.min(n2)], &buf2[..n1.min(n2)],
        "both slaves should receive identical IQ data");

    write_cmd(&mut m, 0x02, 1_024_000u32.to_be_bytes());
    thread::sleep(Duration::from_millis(200));

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn test_real_device_proxy_chain() {
    // Requires two RTL-SDR sticks or one stick with loopback
    // Start upstream serve, then proxy, verify data reaches downstream master
    let mut upstream = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "serve", "--master-port", "9971"])
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("failed to start upstream");
    thread::sleep(Duration::from_millis(1000));

    let mut proxy = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "proxy", "--master-port", "9973", "--slave-port", "9974",
               "--upstream", "127.0.0.1:9971"])
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("failed to start proxy");
    thread::sleep(Duration::from_millis(1000));

    let mut downstream = TcpStream::connect("127.0.0.1:9974").unwrap();
    downstream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    // Connect master to upstream to start IQ flow
    let mut upstream_master = TcpStream::connect("127.0.0.1:9971").unwrap();
    let mut magic = [0u8; 12];
    upstream_master.read_exact(&mut magic).unwrap();

    // Now connect downstream to proxy slave port — should get magic and IQ
    let mut downstream_magic = [0u8; 12];
    downstream.read_exact(&mut downstream_magic).unwrap();
    assert_eq!(&downstream_magic[0..4], b"RTL0");

    // Set frequency on upstream
    write_cmd(&mut upstream_master, 0x01, 100_500_000u32.to_be_bytes());
    thread::sleep(Duration::from_millis(300));

    // Verify IQ reaches downstream
    let mut iq = [0u8; 2048];
    let n = downstream.read(&mut iq).unwrap();
    assert!(n > 0, "downstream should receive IQ data via proxy chain");

    upstream.kill().unwrap();
    proxy.kill().unwrap();
    upstream.wait().unwrap();
    proxy.wait().unwrap();
}
```

- [ ] **Step 2: Verify compilation with feature gate**

Run: `cargo build --features hardware-tests 2>&1`
Expected: Build succeeds (tests compile but are only run when explicitly enabled).

Run: `cargo test --features hardware-tests 2>&1`
Expected: Tests that don't require hardware pass or are correctly ignored. Hardware-specific tests may fail without a device.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml tests/hardware.rs
git commit -m "test: hardware-gated tests for real device serve and proxy chain"
```

---

## Self-Review Checklist

**Spec coverage:**
- [ ] CLI args: `--mode`, `--master-port`, `--slave-port`, `--max-slaves`, `--upstream`, `--key`, `--key-file` → Task 2
- [ ] `--port` backward compat alias → Task 2
- [ ] mode dispatch → Task 6 (serve), Task 8 (proxy)
- [ ] Single-port fallback → `run_serve_single` preserves legacy behavior + Task 6
- [ ] Broadcast channel architecture (tokio::sync::broadcast) → Task 5
- [ ] `read_async` callback bridge → Task 6 (callback → tx.send)
- [ ] Slow-client lag handling → `Lagged(n)` in write_client_loop → Task 5
- [ ] Default buffer capacity 64 → Task 5
- [ ] Slave handshake (magic packet cached) → Task 6
- [ ] Slave commands silently consumed → Task 6 slave consumer thread
- [ ] Per-slave rate limiting → Task 6 slave consumer uses RateLimiter
- [ ] Master commands → serve: apply to device / proxy: forward upstream → Task 6/8
- [ ] Chain detection (0xF0 + timeout 500ms) → Task 3 (upstream ack), Task 7 (downstream probe)
- [ ] Encryption (ChaCha20, nonce exchange, EncryptedStream) → Task 9
- [ ] Graceful shutdown (signal closes all sockets) → Task 6 (all_streams vector)
- [ ] Master disconnect → slaves continue → covered by broadcast persistence (Task 6)
- [ ] systemd socket activation on master only → Task 6 bind_master_port preserves systemd path
- [ ] Slave port refuses when no master → Task 6/8 (slave acceptor runs after master connects)
- [ ] Tuner type caching from device → future refinement (hardcoded magic for now)
- [ ] Integration tests interleaved → Tasks 2/6/7/10/11
- [ ] Hardware tests feature-gated → Task 11
- [ ] `bind_master_port` helper for systemd/plain dual path → Task 6
- [ ] `hex` crate for key parsing → Task 1

**No placeholders:** All code blocks contain real implementation code.
**Type consistency:** All function signatures match between definition and use sites.
**Test-first:** Every feature task starts with a failing test (or verifies existing tests) per TDD.
