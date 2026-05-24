use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rtltcp2026::capture::{self, CaptureChunk, CaptureHeader};

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn create_capture_file(path: &str, chunks: &[CaptureChunk]) {
    let header = CaptureHeader {
        magic_payload: b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d".to_vec(),
    };
    let mut file = std::fs::File::create(path).unwrap();
    capture::write_header(&mut file, &header).unwrap();
    for chunk in chunks {
        capture::write_chunk(&mut file, chunk).unwrap();
    }
    file.flush().unwrap();
}

fn connect_client(port: u16) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(s) = TcpStream::connect(format!("127.0.0.1:{port}")) {
            return s;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for replay to start on port {port}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn make_cap_path(prefix: &str, name: &str) -> String {
    let dir = std::env::temp_dir()
        .join(format!("capture_test_{}", std::process::id()))
        .join(prefix);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_str().unwrap().to_string()
}

#[test]
fn test_replay_magic_and_data() {
    let cap_str = make_cap_path("magic_data", "test.bin");
    let expected_data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
    let chunks = vec![CaptureChunk {
        timestamp_ns: 1000,
        data: expected_data.clone(),
    }];
    create_capture_file(&cap_str, &chunks);

    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .arg(&cap_str)
        .arg("--port").arg(port.to_string())
        .arg("--speed").arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start replay");

    let mut client = connect_client(port);

    let mut magic = [0u8; 12];
    client.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d");

    let mut data = vec![0u8; expected_data.len()];
    client.read_exact(&mut data).unwrap();
    assert_eq!(data, expected_data, "replayed data does not match");

    drop(client);
    let status = child.wait().expect("replay process wait failed");
    assert!(status.success(), "replay exited with error");
}

#[test]
fn test_replay_exits_cleanly_on_disconnect() {
    let cap_str = make_cap_path("disconnect", "test.bin");
    let chunks = vec![CaptureChunk {
        timestamp_ns: 0,
        data: vec![0xDD; 64],
    }];
    create_capture_file(&cap_str, &chunks);

    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .arg(&cap_str)
        .arg("--port").arg(port.to_string())
        .arg("--speed").arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start replay");

    let mut client = connect_client(port);

    let mut magic = [0u8; 12];
    client.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d");

    drop(client);
    let status = child.wait().expect("replay process wait failed");
    assert!(status.success(), "replay exited with error");
}

#[test]
fn test_replay_logs_client_commands() {
    let cap_str = make_cap_path("cmd_log", "test.bin");
    // 50 chunks, 20ms apart, 1KB each = ~1s of streaming at speed 1.0
    let chunks: Vec<_> = (0..50)
        .map(|i| CaptureChunk {
            timestamp_ns: (i as u64) * 20_000_000,
            data: vec![0xCC; 1024],
        })
        .collect();
    create_capture_file(&cap_str, &chunks);

    let port = find_free_port();
    let log_path = std::env::temp_dir()
        .join(format!("replay_cmd_log_{}", std::process::id()));

    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .arg(&cap_str)
        .arg("--port").arg(port.to_string())
        .arg("--speed").arg("1.0")
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(&log_path).unwrap(),
        ))
        .spawn()
        .expect("failed to start replay");

    let mut client = connect_client(port);
    let mut magic = [0u8; 12];
    client.read_exact(&mut magic).unwrap();

    // Drain in background so main thread can write without blocking
    let mut drain_stream = client.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 65536];
        while drain_stream.read(&mut buf).unwrap_or(0) > 0 {}
    });

    // Send commands while main thread sleeps between chunks
    client.write_all(&[0x01, 0x05, 0xFD, 0x82, 0x20]).unwrap();
    client.write_all(&[0x02, 0x00, 0x1F, 0x40, 0x00]).unwrap();
    client.write_all(&[0xF0, 0x00, 0x00, 0x00, 0x00]).unwrap();

    // Wait for stream to finish and logs to flush
    std::thread::sleep(Duration::from_secs(2));

    drop(client);
    let status = child.wait().expect("replay wait failed");
    assert!(status.success(), "replay exited with error");

    let log_content = std::fs::read_to_string(&log_path).unwrap_or_default();
    let _ = std::fs::remove_file(&log_path);
    assert!(log_content.contains("SET_FREQUENCY"), "missing SET_FREQUENCY\n{log_content}");
    assert!(log_content.contains("SET_SAMPLE_RATE"), "missing SET_SAMPLE_RATE\n{log_content}");
    assert!(log_content.contains("CHAIN_DETECT"), "missing CHAIN_DETECT\n{log_content}");
}

#[test]
fn test_replay_missing_file_errors() {
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .arg("/nonexistent/path.bin")
        .arg("--port").arg(port.to_string())
        .arg("--speed").arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn failed");
    assert!(!child.wait().unwrap().success());
}

#[test]
fn test_replay_corrupted_file_errors() {
    let cap_str = make_cap_path("corrupt", "bad.bin");
    std::fs::write(&cap_str, b"NOTANRTLXFILE").unwrap();
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .arg(&cap_str)
        .arg("--port").arg(port.to_string())
        .arg("--speed").arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn failed");
    assert!(!child.wait().unwrap().success());
}

#[test]
fn test_replay_empty_file_errors() {
    let cap_str = make_cap_path("empty", "empty.bin");
    std::fs::write(&cap_str, b"").unwrap();
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .arg(&cap_str)
        .arg("--port").arg(port.to_string())
        .arg("--speed").arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn failed");
    assert!(!child.wait().unwrap().success());
}

#[test]
fn test_capture_connection_failure_errors() {
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-capture"))
        .arg("/tmp/capture_fail.bin")
        .arg("--port").arg(port.to_string())
        .arg("--duration").arg("1")
        .arg("--timeout").arg("1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn failed");
    assert!(!child.wait().unwrap().success());
}

#[test]
fn test_replay_invalid_args_rejected() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp-replay"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn failed");
    assert!(!child.wait().unwrap().success());
}
