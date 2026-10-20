//! Story 5.9 Matrix 32, 39 and the response classification.

use super::*;

const MS: fn(u64) -> Duration = Duration::from_millis;

/// Matrix 32: the hedge delay.
#[test]
fn hedge_timing() {
    let after = MS(1000);
    for (remaining, expected) in [
        (2900, Some(1000)),
        (2000, Some(800)),
        (1500, Some(300)),
        (1400, Some(300)),
        (1200, Some(300)),
        (300, None),
        (200, None),
        (0, None),
    ] {
        assert_eq!(
            hedge_delay(MS(remaining), after),
            expected.map(MS),
            "{remaining}"
        );
    }
    assert_eq!(hedge_delay(MS(2900), MS(300)), Some(MS(300)));
}

/// Matrix 39: the token rule.
#[test]
fn token_rule() {
    for bad in ["", "a/b", "a?b", "a#b", "a b", "é", &"a".repeat(513)] {
        assert!(!valid_token(bad), "{bad:?}");
    }
    assert!(valid_token(&"a".repeat(512)));
    assert!(valid_token("A-z_0.9"));
}

#[test]
fn classify_outcomes() {
    let body = |s: &str| Bytes::from(s.to_owned());
    assert!(matches!(
        classify(StatusCode::NO_CONTENT, &Bytes::new(), None),
        Outcome::Success(CallbackAck {
            status: 204,
            already_acknowledged: false,
            ..
        })
    ));
    assert!(matches!(
        classify(StatusCode::BAD_REQUEST, &body(r#"{"code":40060}"#), None),
        Outcome::Success(CallbackAck {
            already_acknowledged: true,
            ..
        })
    ));
    assert!(matches!(
        classify(StatusCode::BAD_REQUEST, &body(r#"{"code":50035}"#), None),
        Outcome::Rejected {
            status: 400,
            code: Some(50035)
        }
    ));
    assert!(matches!(
        classify(StatusCode::UNAUTHORIZED, &Bytes::new(), None),
        Outcome::Rejected {
            status: 401,
            code: None
        }
    ));
    assert!(matches!(
        classify(StatusCode::SERVICE_UNAVAILABLE, &Bytes::new(), None),
        Outcome::Retryable(CallbackError::Rejected {
            status: 503,
            code: None
        })
    ));
    assert!(matches!(
        classify(StatusCode::TOO_MANY_REQUESTS, &body(r#"{"retry_after":0.5}"#), Some(MS(9000))),
        Outcome::RateLimited(Some(d)) if d == MS(500)
    ));
    assert!(matches!(
        classify(StatusCode::TOO_MANY_REQUESTS, &Bytes::new(), Some(MS(1000))),
        Outcome::RateLimited(Some(d)) if d == MS(1000)
    ));
    assert!(matches!(
        classify(
            StatusCode::TOO_MANY_REQUESTS,
            &body(r#"{"retry_after":-1}"#),
            None
        ),
        Outcome::RateLimited(None)
    ));
}

#[test]
fn request_shape_and_redacted_debug() {
    let request = CallbackRequest {
        interaction_id: Id::new(7),
        token: "tok-1.x_Y".to_owned(),
        body: CallbackBody::Json(Bytes::from_static(b"{}")),
        with_response: true,
        received_at: Instant::now(),
    };
    assert!(!format!("{request:?}").contains("tok-1"));
    let out = outgoing(&request);
    assert_eq!(
        out.target,
        "/api/v10/interactions/7/tok-1.x_Y/callback?with_response=true"
    );
    assert_eq!(out.headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(out.headers[header::CONTENT_LENGTH], "2");
    assert!(
        out.headers[header::USER_AGENT]
            .to_str()
            .unwrap()
            .starts_with("DiscordBot (")
    );
    assert!(out.headers.get(header::AUTHORIZATION).is_none());
}
