//! ChaCha20 encryption for rtltcp proxy chain
//!
//! Provides `EncryptedWriter` and `EncryptedReader` wrappers that
//! transparently encrypt/decrypt a TCP stream using ChaCha20.
//! Also provides `nonce_exchange` for peers to share initialization vectors.

use std::io::{Read, Write};
use chacha20::{ChaCha20, Key, Nonce};
use chacha20::cipher::{KeyIvInit, StreamCipher};

/// Writer wrapper that encrypts all data written through it
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

/// Reader wrapper that decrypts all data read through it
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

/// Generate a random 12-byte nonce using `rand::thread_rng`
pub fn generate_nonce() -> [u8; 12] {
    use rand::Rng;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill(&mut nonce);
    nonce
}

/// Perform a nonce exchange with a peer
///
/// Sends our nonce, receives the peer's nonce, returns both.
/// The shared `key` is not used in this handshake — it is used
/// to construct `EncryptedReader`/`EncryptedWriter` after exchange.
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
    fn test_internal_encrypted_roundtrip() {
        let key = [0xABu8; 32]; let nonce = [0x42u8; 12];
        let (w, r) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut er = EncryptedReader::new(r, key, nonce);
        let data = b"Hello!";
        {
            let mut ew = EncryptedWriter::new(w, key, nonce);
            ew.write_all(data).unwrap();
            ew.flush().unwrap();
        }
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
