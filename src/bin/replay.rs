use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing::{debug, error, info, warn};

use rtltcp2026::capture;
use rtltcp2026::control;

#[derive(Parser, Debug)]
#[clap(name = "rtltcp-replay", about = "Replay a captured RTL_TCP stream")]
struct Args {
    input: String,
    #[clap(long, default_value_t = 1234)]
    port: u16,
    #[clap(long, default_value = "127.0.0.1")]
    bind: String,
    #[clap(long, default_value_t = 1.0)]
    speed: f64,
    #[clap(long = "loop")]
    loop_mode: bool,
}

const HEADER_FIXED_SIZE: u64 = 12;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    let mut file = match std::fs::File::open(&args.input) {
        Ok(f) => f,
        Err(e) => {
            error!("failed to open '{}': {e}", args.input);
            std::process::exit(1);
        }
    };

    let header = match capture::read_header(&mut file) {
        Ok(h) => h,
        Err(e) => {
            error!("failed to read capture header: {e}");
            std::process::exit(1);
        }
    };

    let first_chunk_offset = file
        .stream_position()
        .unwrap_or(HEADER_FIXED_SIZE + header.magic_payload.len() as u64);
    info!(
        "loaded capture: {} bytes magic payload, {} chunks",
        header.magic_payload.len(),
        "?"
    );

    let listener = match TcpListener::bind(format!("{}:{}", args.bind, args.port)) {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind {}:{}: {e}", args.bind, args.port);
            std::process::exit(1);
        }
    };

    info!("listening on {}:{}", args.bind, args.port);
    let (mut client, addr) = match listener.accept() {
        Ok(c) => c,
        Err(e) => {
            error!("accept failed: {e}");
            std::process::exit(1);
        }
    };

    info!("client connected from {addr}");

    if let Err(e) = client.write_all(&header.magic_payload) {
        error!("failed to send magic packet: {e}");
        std::process::exit(1);
    }

    let client_quit = Arc::new(AtomicBool::new(false));
    let cmd_quit = client_quit.clone();
    let mut cmd_stream = match client.try_clone() {
        Ok(s) => s,
        Err(e) => {
            error!("failed to clone client stream: {e}");
            std::process::exit(1);
        }
    };

    let _ = cmd_stream.set_read_timeout(Some(Duration::from_millis(200)));
    let reader_quit = cmd_quit.clone();
    let cmd_thread = std::thread::spawn(move || {
        let mut buf = [0u8; control::COMMAND_HEADER_SIZE];
        loop {
            match cmd_stream.read_exact(&mut buf) {
                Ok(()) => {
                    let cmd = buf[0];
                    let payload_hex = hex::encode(&buf[1..]);
                    match cmd {
                        control::CMD_SET_FREQUENCY => {
                            let freq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
                            info!("client command: SET_FREQUENCY freq={freq}");
                        }
                        control::CMD_SET_SAMPLE_RATE => {
                            let rate = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
                            info!("client command: SET_SAMPLE_RATE rate={rate}");
                        }
                        control::CMD_SET_GAIN_MODE => {
                            let mode = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
                            info!("client command: SET_GAIN_MODE mode={mode}");
                        }
                        control::CMD_SET_TUNER_GAIN => {
                            let gain = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
                            info!("client command: SET_TUNER_GAIN gain={gain}");
                        }
                        control::CMD_SET_PPM => {
                            let ppm = i32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
                            info!("client command: SET_PPM ppm={ppm}");
                        }
                        control::CMD_SET_AGC => {
                            let agc = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
                            info!("client command: SET_AGC agc={agc}");
                        }
                        control::CMD_CHAIN_DETECT => {
                            info!("client command: CHAIN_DETECT");
                        }
                        _ => {
                            info!("client command: UNKNOWN payload={payload_hex}");
                        }
                    }
                }
                Err(ref e) if is_disconnect_err(e) => {
                    debug!("client disconnected (command reader)");
                    break;
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    if reader_quit.load(Ordering::SeqCst) {
                        break;
                    }
                    continue;
                }
                Err(e) => {
                    warn!("command read error: {e}");
                    break;
                }
            }
        }
    });

    let mut prev_timestamp: Option<u64> = None;
    loop {
        let chunk = match capture::read_chunk(&mut file) {
            Ok(Some(c)) => c,
            Ok(None) => {
                if args.loop_mode {
                    info!("end of capture, looping");
                    if let Err(e) = file.seek(SeekFrom::Start(first_chunk_offset)) {
                        error!("seek failed: {e}");
                        break;
                    }
                    prev_timestamp = None;
                    continue;
                }
                info!("end of capture, done");
                break;
            }
            Err(e) => {
                error!("read chunk error: {e}");
                break;
            }
        };

        if client_quit.load(Ordering::SeqCst) {
            break;
        }

        if args.speed > 0.0 {
            if let Some(prev) = prev_timestamp {
                let delta_ns = ((chunk.timestamp_ns - prev) as f64 / args.speed) as u64;
                if delta_ns > 0 {
                    std::thread::sleep(Duration::from_nanos(delta_ns));
                }
            }
            prev_timestamp = Some(chunk.timestamp_ns);
        }

        if let Err(e) = client.write_all(&chunk.data) {
            if is_disconnect_err(&e) {
                info!("client disconnected");
            } else {
                warn!("write error: {e}");
            }
            break;
        }
    }

    client_quit.store(true, Ordering::SeqCst);
    let _ = cmd_thread.join();
    info!("replay complete");
}

fn is_disconnect_err(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
    )
}
