use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tracing::{error, info};

use rtltcp2026::capture::{self, CaptureChunk, CaptureHeader};

#[derive(Parser, Debug)]
#[clap(
    name = "rtltcp-capture",
    about = "Capture IQ data from an RTL_TCP server to a file"
)]
struct Args {
    output: String,
    #[clap(long, default_value = "127.0.0.1")]
    host: String,
    #[clap(long, default_value_t = 1234)]
    port: u16,
    #[clap(long, default_value_t = 10)]
    duration: u64,
    #[clap(long, default_value_t = 1)]
    timeout: u64,
    #[clap(long, default_value_t = 67108864)]
    buffer_mem: u64,
}

fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let should_exit = Arc::new(AtomicBool::new(false));

    ctrlc::set_handler({
        let e = should_exit.clone();
        move || {
            info!("received interrupt, flushing...");
            e.store(true, Ordering::SeqCst);
        }
    })
    .expect("failed to set signal handler");

    let addr = format!("{}:{}", args.host, args.port);
    info!("connecting to {addr}");
    let mut stream = match TcpStream::connect(&addr) {
        Ok(s) => s,
        Err(e) => {
            error!("failed to connect to {addr}: {e}");
            std::process::exit(1);
        }
    };

    stream
        .set_read_timeout(Some(Duration::from_secs(args.timeout)))
        .ok();

    let mut magic_payload = vec![0u8; 12];
    if let Err(e) = stream.read_exact(&mut magic_payload) {
        error!("failed to read magic packet: {e}");
        std::process::exit(1);
    }

    info!("received magic packet, starting capture");

    let mut file = match std::fs::File::create(&args.output) {
        Ok(f) => f,
        Err(e) => {
            error!("failed to create output file '{}': {e}", args.output);
            std::process::exit(1);
        }
    };

    let header = CaptureHeader {
        magic_payload: magic_payload.clone(),
    };
    if let Err(e) = capture::write_header(&mut file, &header) {
        error!("failed to write header: {e}");
        std::process::exit(1);
    }

    let start = Instant::now();
    let mut total_bytes: u64 = 0;
    let mut buf = Vec::new();

    loop {
        if should_exit.load(Ordering::SeqCst) {
            break;
        }

        let elapsed = start.elapsed();
        if elapsed >= Duration::from_secs(args.duration) {
            info!("duration reached, stopping capture");
            break;
        }

        let mut chunk_data = vec![0u8; 512 * 1024];
        let n = match stream.read(&mut chunk_data) {
            Ok(0) => {
                info!("server closed connection");
                break;
            }
            Ok(n) => n,
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                error!("read error: {e}");
                break;
            }
        };
        chunk_data.truncate(n);

        let timestamp_ns = start.elapsed().as_nanos() as u64;
        let chunk = CaptureChunk {
            timestamp_ns,
            data: chunk_data,
        };

        let mut chunk_buf = Vec::new();
        if capture::write_chunk(&mut chunk_buf, &chunk).is_ok() {
            total_bytes += n as u64;
            buf.extend_from_slice(&chunk_buf);

            if buf.len() >= args.buffer_mem as usize {
                if let Err(e) = file.write_all(&buf) {
                    error!("flush write failed: {e}");
                    break;
                }
                info!(
                    "flushed {} bytes to disk ({} total)",
                    buf.len(),
                    total_bytes
                );
                buf.clear();
            }
        }
    }

    if !buf.is_empty() {
        if let Err(e) = file.write_all(&buf) {
            error!("final flush write failed: {e}");
        }
    }

    let _ = file.flush();
    let elapsed = start.elapsed();
    let file_size = std::fs::metadata(&args.output)
        .map(|m| m.len())
        .unwrap_or(0);

    info!("capture complete");
    info!("  total bytes captured: {total_bytes}");
    info!("  elapsed time: {:.3}s", elapsed.as_secs_f64());
    info!("  file size: {file_size} bytes");
}
