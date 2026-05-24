use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use tokio::sync::broadcast;
use tracing::{debug, warn};

pub const DEFAULT_BROADCAST_CAPACITY: usize = 64;

pub fn new_broadcast(
    capacity: usize,
) -> (broadcast::Sender<Vec<u8>>, broadcast::Receiver<Vec<u8>>) {
    broadcast::channel(capacity)
}

/// Writer loop for a single client: reads from broadcast, writes to TCP.
/// Exits when `should_exit` is set, channel is closed, or write errors.
pub fn write_client_loop(
    mut stream: impl Write,
    mut rx: broadcast::Receiver<Vec<u8>>,
    should_exit: &AtomicBool,
) {
    loop {
        if should_exit.load(Ordering::SeqCst) {
            debug!("writer thread: exit flag set, stopping");
            break;
        }

        match rx.try_recv() {
            Ok(buf) => {
                if let Err(e) = stream.write_all(&buf) {
                    warn!("writer thread: write error, stopping: {e}");
                    break;
                }
            }
            Err(broadcast::error::TryRecvError::Empty) => {
                thread::sleep(Duration::from_micros(100));
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                debug!("writer thread: broadcast closed, stopping");
                break;
            }
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                warn!("writer thread: lagged by {n} buffers, continuing");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::Arc;

    #[test]
    fn test_new_broadcast_send_recv() {
        let (tx, mut rx) = new_broadcast(16);
        let data = vec![1u8, 2u8, 3u8];
        tx.send(data.clone()).unwrap();
        assert_eq!(rx.try_recv().unwrap(), data);
    }

    #[test]
    fn test_broadcast_multiple_receivers() {
        let (tx, _rx) = new_broadcast(16);
        let mut rx1 = tx.subscribe();
        let mut rx2 = tx.subscribe();
        let data = vec![0xABu8; 256];
        tx.send(data.clone()).unwrap();
        assert_eq!(rx1.try_recv().unwrap(), data);
        assert_eq!(rx2.try_recv().unwrap(), data);
    }

    #[test]
    fn test_writer_exits_on_flag() {
        let (tx, _rx) = new_broadcast(16);
        let rx = tx.subscribe();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();
        let (mut r, w) = std::os::unix::net::UnixStream::pair().unwrap();
        let h = thread::spawn(move || write_client_loop(w, rx, &flag_clone));
        tx.send(vec![0x42; 32]).unwrap();
        thread::sleep(Duration::from_millis(10));
        let mut buf = vec![0u8; 32];
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf, vec![0x42; 32]);
        flag.store(true, Ordering::SeqCst);
        h.join().unwrap();
    }
}
