//! Hardware-gated integration tests
//!
//! These tests require a physical RTL-SDR device and are only compiled
//! when the `hardware-tests` feature is enabled:
//!
//!     cargo test --features hardware-tests -- --ignored

#![cfg(feature = "hardware-tests")]

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
fn test_real_device_serve_multi() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args([
            "--mode",
            "serve",
            "--master-port",
            "9981",
            "--slave-port",
            "9982",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start binary");
    thread::sleep(Duration::from_millis(1000));

    let mut m = TcpStream::connect("127.0.0.1:9981").unwrap();
    let mut s1 = TcpStream::connect("127.0.0.1:9982").unwrap();
    let mut s2 = TcpStream::connect("127.0.0.1:9982").unwrap();

    m.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s1.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s2.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    // All get magic packet
    let mut mm = [0u8; 12];
    m.read_exact(&mut mm).unwrap();
    let mut s1m = [0u8; 12];
    s1.read_exact(&mut s1m).unwrap();
    let mut s2m = [0u8; 12];
    s2.read_exact(&mut s2m).unwrap();
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
    assert_eq!(
        &buf1[..n1.min(n2)],
        &buf2[..n1.min(n2)],
        "both slaves should receive identical IQ data"
    );

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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start upstream");
    thread::sleep(Duration::from_millis(1000));

    let mut proxy = Command::new(env!("CARGO_BIN_EXE_rtltcp2026"))
        .args([
            "--mode",
            "proxy",
            "--master-port",
            "9973",
            "--slave-port",
            "9974",
            "--upstream",
            "127.0.0.1:9971",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start proxy");
    thread::sleep(Duration::from_millis(1000));

    let mut downstream = TcpStream::connect("127.0.0.1:9974").unwrap();
    downstream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

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
