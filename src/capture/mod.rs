use std::io::{self, Read, Seek, SeekFrom, Write};

pub const CAPTURE_MAGIC: &[u8; 4] = b"RTLX";
pub const CAPTURE_VERSION: u32 = 1;

pub struct CaptureHeader {
    pub magic_payload: Vec<u8>,
}

pub struct CaptureChunk {
    pub timestamp_ns: u64,
    pub data: Vec<u8>,
}

pub fn write_header<W: Write + Seek>(writer: &mut W, header: &CaptureHeader) -> io::Result<()> {
    let start = writer.stream_position()?;
    writer.write_all(CAPTURE_MAGIC)?;
    writer.write_all(&CAPTURE_VERSION.to_le_bytes())?;
    let magic_len = header.magic_payload.len() as u32;
    writer.write_all(&magic_len.to_le_bytes())?;
    writer.write_all(&header.magic_payload)?;
    let end = writer.stream_position()?;
    if start != 0 {
        let header_bytes = write_header_to_vec(header)?;
        writer.seek(SeekFrom::Start(0))?;
        writer.write_all(&header_bytes)?;
        writer.seek(SeekFrom::Start(end))?;
    }
    Ok(())
}

fn write_header_to_vec(header: &CaptureHeader) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.extend_from_slice(CAPTURE_MAGIC);
    buf.extend_from_slice(&CAPTURE_VERSION.to_le_bytes());
    let magic_len = header.magic_payload.len() as u32;
    buf.extend_from_slice(&magic_len.to_le_bytes());
    buf.extend_from_slice(&header.magic_payload);
    Ok(buf)
}

pub fn read_header<R: Read>(reader: &mut R) -> io::Result<CaptureHeader> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != CAPTURE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid capture magic"));
    }
    let mut version_buf = [0u8; 4];
    reader.read_exact(&mut version_buf)?;
    let _version = u32::from_le_bytes(version_buf);
    let mut magic_len_buf = [0u8; 4];
    reader.read_exact(&mut magic_len_buf)?;
    let magic_len = u32::from_le_bytes(magic_len_buf) as usize;
    let mut magic_payload = vec![0u8; magic_len];
    reader.read_exact(&mut magic_payload)?;
    Ok(CaptureHeader { magic_payload })
}

pub fn write_chunk<W: Write>(writer: &mut W, chunk: &CaptureChunk) -> io::Result<()> {
    writer.write_all(&chunk.timestamp_ns.to_le_bytes())?;
    let data_len = chunk.data.len() as u32;
    writer.write_all(&data_len.to_le_bytes())?;
    writer.write_all(&chunk.data)
}

pub fn read_chunk<R: Read>(reader: &mut R) -> io::Result<Option<CaptureChunk>> {
    let mut ts_buf = [0u8; 8];
    match reader.read_exact(&mut ts_buf) {
        Ok(()) => {}
        Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let timestamp_ns = u64::from_le_bytes(ts_buf);
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let data_len = u32::from_le_bytes(len_buf) as usize;
    let mut data = vec![0u8; data_len];
    reader.read_exact(&mut data)?;
    Ok(Some(CaptureChunk { timestamp_ns, data }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_roundtrip_header_and_chunks() {
        let header = CaptureHeader {
            magic_payload: b"RTL0\x00\x00\x00\x05\x00\x00\x00\x1d".to_vec(),
        };

        let chunks = vec![
            CaptureChunk { timestamp_ns: 1000, data: vec![0u8; 64] },
            CaptureChunk { timestamp_ns: 2000, data: vec![1u8; 128] },
            CaptureChunk { timestamp_ns: 3000, data: vec![2u8; 256] },
        ];

        let mut cursor = Cursor::new(Vec::new());
        write_header(&mut cursor, &header).unwrap();
        for chunk in &chunks {
            write_chunk(&mut cursor, chunk).unwrap();
        }

        cursor.set_position(0);
        let got_header = read_header(&mut cursor).unwrap();
        assert_eq!(got_header.magic_payload, header.magic_payload);

        for expected in &chunks {
            let got = read_chunk(&mut cursor).unwrap().expect("expected chunk");
            assert_eq!(got.timestamp_ns, expected.timestamp_ns);
            assert_eq!(got.data, expected.data);
        }
        assert!(read_chunk(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn test_roundtrip_empty_chunk() {
        let header = CaptureHeader { magic_payload: vec![0u8; 12] };
        let chunks = vec![
            CaptureChunk { timestamp_ns: 42, data: vec![] },
            CaptureChunk { timestamp_ns: 99, data: vec![0xAB; 1] },
        ];

        let mut cursor = Cursor::new(Vec::new());
        write_header(&mut cursor, &header).unwrap();
        for chunk in &chunks {
            write_chunk(&mut cursor, chunk).unwrap();
        }

        cursor.set_position(0);
        let _ = read_header(&mut cursor).unwrap();
        let first = read_chunk(&mut cursor).unwrap().unwrap();
        assert_eq!(first.timestamp_ns, 42);
        assert!(first.data.is_empty());
        let second = read_chunk(&mut cursor).unwrap().unwrap();
        assert_eq!(second.data, vec![0xAB]);
    }

    #[test]
    fn test_buffer_flush_identity() {
        let header = CaptureHeader { magic_payload: vec![0u8; 12] };
        let chunks: Vec<_> = (0..10).map(|i| CaptureChunk {
            timestamp_ns: i as u64 * 1000,
            data: vec![i as u8; 32],
        }).collect();

        // Write all at once (unbuffered)
        let mut all_at_once = Cursor::new(Vec::new());
        write_header(&mut all_at_once, &header).unwrap();
        for chunk in &chunks {
            write_chunk(&mut all_at_once, chunk).unwrap();
        }

        // Write with flushes at arbitrary boundaries (simulating buffer)
        let mut with_flush = Cursor::new(Vec::new());
        write_header(&mut with_flush, &header).unwrap();
        let mut buf = Vec::new();
        for chunk in &chunks {
            let mut chunk_buf = Vec::new();
            write_chunk(&mut chunk_buf, chunk).unwrap();
            buf.extend_from_slice(&chunk_buf);
            if buf.len() > 50 {
                with_flush.write_all(&buf).unwrap();
                buf.clear();
            }
        }
        if !buf.is_empty() {
            with_flush.write_all(&buf).unwrap();
        }

        assert_eq!(all_at_once.into_inner(), with_flush.into_inner());
    }

    #[test]
    fn test_invalid_magic_rejected() {
        let mut cursor = Cursor::new(b"XXXX\x01\x00\x00\x00\x0c\x00\x00\x00RTL0\x00\x00\x00\x05\x00\x00\x00\x1d" as &[u8]);
        let result = read_header(&mut cursor);
        assert!(result.is_err());
    }
}
