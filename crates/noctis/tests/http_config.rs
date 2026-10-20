//! Story 5.9 Matrix 52: the HTTP edge's public types resolve at the facade
//! root.

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use std::time::Duration;

use noctis::{AckDeadlineMissed, BuildViolation, HttpConfig, HttpConfigError};

#[test]
fn facade_types() {
    let config = HttpConfig::default();
    assert_eq!(config.hedge_after, Duration::from_secs(1));
    let error = HttpConfigError::PingTimeout(Duration::ZERO);
    assert_eq!(
        BuildViolation::InvalidHttpConfig { reason: error }.to_string(),
        "invalid HTTP config: HttpConfig.ping_timeout 0ns is out of range: use 50 ms..=60 s"
    );
    let missed: fn(&AckDeadlineMissed) -> String = ToString::to_string;
    let _ = missed;
}

#[tokio::test]
async fn ack_deadline_missed_message() {
    // `AckDeadlineMissed` is non-exhaustive; the edge builds it. Reach one
    // through the test seam.
    let app = noctis::App::builder().build().unwrap();
    let received_at = tokio::time::Instant::now() - Duration::from_secs(4);
    let error = noctis_core::__testing::interaction_callback(
        &app,
        1,
        "t",
        bytes::Bytes::new(),
        false,
        received_at,
    )
    .await
    .unwrap_err();
    let noctis_core::__testing::CallbackError::DeadlineMissed(missed) = error else {
        panic!("{error:?}");
    };
    let missed: AckDeadlineMissed = missed;
    assert!(missed.elapsed >= Duration::from_secs(4));
    let text = missed.to_string();
    assert!(
        text.starts_with("interaction not acknowledged in time: 4."),
        "{text}"
    );
    assert!(
        text.ends_with(
            " after receipt, past the 2.9 s ack deadline (Discord allows 3 s; INT-01): defer slow work"
        ),
        "{text}"
    );
}
