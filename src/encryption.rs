//! Encryption module — stub for proxy compilation
pub fn nonce_exchange(
    _stream: &mut (impl std::io::Read + std::io::Write),
    _key: [u8; 32],
) -> std::io::Result<([u8; 12], [u8; 12])> {
    Err(std::io::Error::new(std::io::ErrorKind::Other, "not implemented"))
}
