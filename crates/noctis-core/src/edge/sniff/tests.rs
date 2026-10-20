//! Story 5.9 Matrix 46: the PING scanner and tracker.

use std::sync::{Arc, Mutex};

use super::*;

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len()).unwrap();
    let mut out = len.to_be_bytes()[1..].to_vec();
    out.extend([kind, flags]);
    out.extend(stream.to_be_bytes());
    out.extend(payload);
    out
}

fn ping(ack: bool, payload: [u8; 8]) -> Vec<u8> {
    frame(PING, if ack { ACK } else { 0 }, 0, &payload)
}

/// Feeds `bytes` split at `at` and collects the PINGs.
fn scan(mut scanner: FrameScanner, bytes: &[u8], at: usize) -> Vec<(bool, [u8; 8])> {
    let mut seen = Vec::new();
    let (a, b) = bytes.split_at(at);
    scanner.feed(a, |ack, p| seen.push((ack, p)));
    scanner.feed(b, |ack, p| seen.push((ack, p)));
    seen
}

#[test]
fn outbound_split_everywhere() {
    let mut bytes = PREFACE.to_vec();
    bytes.extend(frame(0x4, 0, 0, &[0, 3, 0, 0, 0, 100]));
    bytes.extend(frame(0x0, 0, 1, &vec![7; 16 * 1024]));
    bytes.extend(frame(0x4, ACK, 0, &[]));
    bytes.extend(ping(false, [1; 8]));
    for at in 0..=bytes.len() {
        assert_eq!(
            scan(FrameScanner::outbound(), &bytes, at),
            [(false, [1; 8])],
            "split at {at}"
        );
    }
}

#[test]
fn inbound_ack_split_everywhere() {
    let mut bytes = frame(0x4, 0, 0, &[]);
    bytes.extend(ping(true, [9; 8]));
    for at in 0..=bytes.len() {
        assert_eq!(scan(FrameScanner::inbound(), &bytes, at), [(true, [9; 8])]);
    }
}

#[test]
fn byte_by_byte_and_garbage_never_panic() {
    let mut scanner = FrameScanner::inbound();
    let mut count = 0;
    for byte in ping(true, [2; 8]) {
        scanner.feed(&[byte], |_, _| count += 1);
    }
    assert_eq!(count, 1);
    // A PING with a wrong length is skipped, not captured.
    let mut scanner = FrameScanner::inbound();
    scanner.feed(&frame(PING, 0, 0, &[1; 9]), |_, _| count += 1);
    scanner.feed(&[0xff; 4096], |_, _| {});
    assert_eq!(count, 1);
}

#[tokio::test(start_paused = true)]
async fn tracker_matches_and_evicts() {
    let samples = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&samples);
    let tracker = PingTracker::new(move |rtt| sink.lock().unwrap().push(rtt));
    tracker.sent([1; 8]);
    tokio::time::advance(Duration::from_millis(30)).await;
    tracker.acked([9; 8]);
    assert!(samples.lock().unwrap().is_empty(), "unknown ACK p9");
    tracker.acked([1; 8]);
    assert_eq!(*samples.lock().unwrap(), [Duration::from_millis(30)]);
    for n in 0..5 {
        tracker.sent([n; 8]);
    }
    assert_eq!(tracker.outstanding(), 4);
    tracker.acked([0; 8]);
    assert_eq!(samples.lock().unwrap().len(), 1, "the oldest was evicted");
    tracker.acked([4; 8]);
    assert_eq!(samples.lock().unwrap().len(), 2);
}
