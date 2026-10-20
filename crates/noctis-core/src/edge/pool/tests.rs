//! Story 5.9 Matrix 20: the redial back-off.

use super::*;

/// Matrix 20: 0, 0.5, 1, 2, 5, 5 s, each within the jitter band, and a
/// reset after a success (a new `Backoff`).
#[test]
fn backoff_steps_and_jitter() {
    let bands = [
        (0, 0),
        (400, 600),
        (800, 1200),
        (1600, 2400),
        (4000, 6000),
        (4000, 6000),
    ];
    for jitter in [0.8, 1.0, 1.2] {
        let mut backoff = Backoff::default();
        for (low, high) in bands {
            let delay = backoff.next_delay(jitter);
            assert!(
                (Duration::from_millis(low)..=Duration::from_millis(high)).contains(&delay),
                "{delay:?} not in {low}..={high} ms"
            );
        }
        let mut reset = Backoff::default();
        assert_eq!(reset.next_delay(jitter), Duration::ZERO);
    }
}

#[test]
fn jitter_band() {
    for _ in 0..1000 {
        let j = jitter();
        assert!((0.8..=1.2).contains(&j), "{j}");
    }
}
