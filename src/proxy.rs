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
