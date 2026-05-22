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
