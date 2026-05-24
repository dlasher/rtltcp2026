//! End-to-end chain integration tests
//!
//! Tests multi-client serve mode behavior including:
//! - Invalid mode rejection
//! - Slave command silent consumption
//! - (Hardware-gated) Master/slave same IQ stream

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn write_cmd(s: &mut TcpStream, opcode: u8, payload: [u8; 4]) {
    s.write_all(&[opcode, payload[0], payload[1], payload[2], payload[3]])
        .unwrap();
}

#[test]
fn test_invalid_mode_fails() {
    let o = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args(["--mode", "invalid"])
        .output()
        .unwrap();
    assert!(!o.status.success());
}

#[test]
#[ignore] // Requires USB device
fn test_master_slave_same_iq() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args([
            "--mode",
            "serve",
            "--master-port",
            "9941",
            "--slave-port",
            "9942",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(500));

    let mut master = TcpStream::connect("127.0.0.1:9941").unwrap();
    let mut slave = TcpStream::connect("127.0.0.1:9942").unwrap();
    master
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    slave
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let mut mm = [0u8; 12];
    master.read_exact(&mut mm).unwrap();
    let mut sm = [0u8; 12];
    slave.read_exact(&mut sm).unwrap();
    assert_eq!(mm, sm);

    write_cmd(&mut master, 0x01, 100_500_000u32.to_be_bytes());
    thread::sleep(Duration::from_millis(200));

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
#[ignore] // Requires USB device (server opens device before accepting master)
fn test_slave_command_silently_consumed() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args([
            "--mode",
            "serve",
            "--master-port",
            "9943",
            "--slave-port",
            "9944",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(300));

    let _master = TcpStream::connect("127.0.0.1:9943").unwrap();
    let mut slave = TcpStream::connect("127.0.0.1:9944").unwrap();

    // Read magic packet
    let mut sm = [0u8; 12];
    slave.read_exact(&mut sm).unwrap();

    // Send various commands from slave — should be silently consumed
    write_cmd(&mut slave, 0x01, 100_000_000u32.to_be_bytes());
    write_cmd(&mut slave, 0xFF, [0; 4]);
    write_cmd(&mut slave, 0x00, [0; 4]);
    thread::sleep(Duration::from_millis(100));

    // Slave should NOT receive any response to its commands
    slave
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let mut response_buf = [0u8; 1];
    let read_result = slave.read(&mut response_buf);
    assert!(
        read_result.is_err(),
        "slave should not receive data after sending commands"
    );

    child.kill().unwrap();
    child.wait().unwrap();
}
