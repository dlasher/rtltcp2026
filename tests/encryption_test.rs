use std::io::{Read, Write};
use std::thread;

#[test]
fn test_encrypted_roundtrip() {
    let key = [0xABu8; 32];
    let nonce = [0x42u8; 12];

    let (w, r) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut enc_r = rtltcp2026::encryption::EncryptedReader::new(r, key, nonce);

    let data = b"Hello, ChaCha20!";
    {
        let mut enc_w = rtltcp2026::encryption::EncryptedWriter::new(w, key, nonce);
        enc_w.write_all(data).unwrap();
        enc_w.flush().unwrap();
    }

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

#[test]
fn test_nonce_exchange_works() {
    let (mut a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
    let key = [0x01u8; 32];
    let h = thread::spawn(move || rtltcp2026::encryption::nonce_exchange(&mut b, key).unwrap());
    let (my_a, peer_a) = rtltcp2026::encryption::nonce_exchange(&mut a, key).unwrap();
    let (my_b, peer_b) = h.join().unwrap();
    assert_eq!(my_a, peer_b);
    assert_eq!(my_b, peer_a);
}
