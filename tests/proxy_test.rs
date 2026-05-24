use chacha20::cipher::{KeyIvInit, StreamCipher};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Fake upstream that responds to 0xF0 with ack
fn start_upstream_chain() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d")
            .unwrap();
        let mut buf = [0u8; 5];
        s.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0xF0);
        s.write_all(&[0xF0, 0x00, 0x00, 0x00, 0x00]).unwrap();
    });
    p
}

#[test]
fn test_chain_detect_handshake() {
    let p = start_upstream_chain();
    thread::sleep(Duration::from_millis(50));
    let result =
        rtltcp2026::proxy::connect_upstream("127.0.0.1", p, None, Duration::from_millis(500));
    assert!(
        result.is_ok(),
        "connect_upstream should succeed with ack upstream"
    );
    let conn = result.unwrap();
    assert!(conn.is_chain, "should detect chain mode");
    assert!(conn.write_cipher.is_none(), "no key provided");
}

#[test]
fn test_chain_detect_timeout_gives_plain() {
    // Standard rtltcp: sends magic but doesn't respond to 0xF0
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d")
            .unwrap();
        let mut buf = [0u8; 5];
        s.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0xF0);
        thread::sleep(Duration::from_millis(600));
    });

    thread::sleep(Duration::from_millis(50));
    let result =
        rtltcp2026::proxy::connect_upstream("127.0.0.1", p, None, Duration::from_millis(200));
    assert!(result.is_ok(), "should fall back gracefully on timeout");
    let conn = result.unwrap();
    assert!(!conn.is_chain, "no chain without ack");
}

#[test]
fn test_encrypted_proxy_command_roundtrip() {
    let key = [0x42u8; 32];
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    let (handshake_done_tx, handshake_done_rx) = mpsc::channel();

    let server_key = key;
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d")
            .unwrap();
        let mut buf = [0u8; 5];
        s.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0xF0);
        s.write_all(&[0xF0, 0x00, 0x00, 0x00, 0x00]).unwrap();
        let my_nonce = rtltcp2026::encryption::generate_nonce();
        s.write_all(&my_nonce).unwrap();
        let mut peer_nonce = [0u8; 12];
        s.read_exact(&mut peer_nonce).unwrap();
        let mut read_cipher = chacha20::ChaCha20::new(
            chacha20::Key::from_slice(&server_key),
            chacha20::Nonce::from_slice(&peer_nonce),
        );
        handshake_done_tx.send(()).ok();

        let expected_cmds: [[u8; 5]; 3] = [
            [0x01, 0x11, 0x22, 0x33, 0x44],
            [0x02, 0xAA, 0xBB, 0xCC, 0xDD],
            [0x03, 0x00, 0xFF, 0x00, 0xFF],
        ];
        for expected in &expected_cmds {
            let mut cmd_buf = [0u8; 5];
            s.read_exact(&mut cmd_buf).unwrap();
            read_cipher.apply_keystream(&mut cmd_buf);
            assert_eq!(
                &cmd_buf, expected,
                "server should decrypt forwarded command"
            );
        }
    });

    let mut conn =
        rtltcp2026::proxy::connect_upstream("127.0.0.1", p, Some(key), Duration::from_millis(500))
            .unwrap();
    assert!(conn.is_chain, "should detect chain mode");
    handshake_done_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    let mut write_cipher = conn
        .write_cipher
        .expect("write_cipher should be set when key provided");

    let cmds: [[u8; 5]; 3] = [
        [0x01, 0x11, 0x22, 0x33, 0x44],
        [0x02, 0xAA, 0xBB, 0xCC, 0xDD],
        [0x03, 0x00, 0xFF, 0x00, 0xFF],
    ];
    for cmd in &cmds {
        let mut encrypted = *cmd;
        write_cipher.apply_keystream(&mut encrypted);
        conn.stream.write_all(&encrypted).unwrap();
    }
}
