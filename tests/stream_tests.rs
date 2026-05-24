use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_broadcast_send_recv() {
    let (tx, _rx) = rtltcp2026::stream::new_broadcast(16);
    let mut rx1 = tx.subscribe();
    let mut rx2 = tx.subscribe();

    let data = vec![0u8; 512];
    assert!(tx.send(data.clone()).is_ok());

    let recv1 = rx1.try_recv().unwrap();
    let recv2 = rx2.try_recv().unwrap();
    assert_eq!(recv1.len(), 512);
    assert_eq!(recv2, recv1);
}

#[test]
fn test_broadcast_lag() {
    use tokio::sync::broadcast::error::TryRecvError;
    let (tx, _rx) = rtltcp2026::stream::new_broadcast(4);
    let mut rx = tx.subscribe();

    for _ in 0..4 {
        let _ = tx.send(vec![0u8; 64]);
    }
    let _ = tx.send(vec![1u8; 64]);

    match rx.try_recv() {
        Err(TryRecvError::Lagged(n)) => assert!(n > 0),
        other => panic!("expected Lagged, got {other:?}"),
    }
}

#[test]
fn test_writer_loop_exits_on_flag() {
    let (tx, _rx) = rtltcp2026::stream::new_broadcast(16);
    let rx = tx.subscribe();
    let should_exit = Arc::new(AtomicBool::new(false));

    let (mut reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();

    let we = should_exit.clone();
    let handle = thread::spawn(move || {
        rtltcp2026::stream::write_client_loop(writer, rx, &we);
    });

    tx.send(vec![0x42u8; 32]).unwrap();
    thread::sleep(Duration::from_millis(20));

    let mut buf = vec![0u8; 32];
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(buf, vec![0x42u8; 32]);

    should_exit.store(true, Ordering::SeqCst);
    handle.join().unwrap();
}
