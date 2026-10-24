//! Story 12.4 Matrix R1-R15: the pure 429 rules.

use super::*;

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(*name, HeaderValue::from_static(value));
    }
    map
}

fn with_scope_bytes(bytes: &[u8]) -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert(SCOPE, HeaderValue::from_bytes(bytes).unwrap());
    map
}

/// R1, R2.
#[test]
fn scope_is_exact() {
    for (value, expected) in [
        ("user", RateLimitScope::User),
        ("global", RateLimitScope::Global),
        ("shared", RateLimitScope::Shared),
    ] {
        assert_eq!(scope(&headers(&[(SCOPE, value)])), expected, "{value}");
    }
    assert_eq!(scope(&HeaderMap::new()), RateLimitScope::Unknown);
    for bytes in [
        &b""[..],
        b"User",
        b" shared",
        b"shared ",
        "shar\u{e9}d".as_bytes(),
        b"\xff",
    ] {
        assert_eq!(
            scope(&with_scope_bytes(bytes)),
            RateLimitScope::Unknown,
            "{bytes:?}"
        );
    }
}

/// R3.
#[test]
fn scope_less_global_body() {
    let limited = classify_429(&HeaderMap::new(), br#"{"global":true,"retry_after":1.5}"#);
    assert_eq!(limited.scope, RateLimitScope::Global);
    assert_eq!(limited.retry_after, Some(Duration::from_millis(1500)));
    // A header scope wins over the body flag.
    let limited = classify_429(&headers(&[(SCOPE, "shared")]), br#"{"global":true}"#);
    assert_eq!(limited.scope, RateLimitScope::Shared);
}

/// R4.
#[test]
fn body_retry_after() {
    for (raw, expected) in [
        ("0", Duration::ZERO),
        ("0.001", Duration::from_millis(1)),
        ("64.57", Duration::from_secs_f64(64.57)),
    ] {
        let body = format!(r#"{{"retry_after":{raw}}}"#);
        let limited = classify_429(&HeaderMap::new(), body.as_bytes());
        assert_eq!(limited.retry_after, Some(expected), "{raw}");
    }
}

/// R5: an unusable body value falls back to the header.
#[test]
fn header_retry_after_fallback() {
    let header = headers(&[("retry-after", "2")]);
    for body in [
        r#"{"retry_after":-1}"#,
        r#"{"retry_after":"1"}"#,
        r#"{"retry_after":1e400}"#,
        r#"{"retry_after":1e300}"#,
        r#"{"message":"x"}"#,
    ] {
        let limited = classify_429(&header, body.as_bytes());
        assert_eq!(limited.retry_after, Some(Duration::from_secs(2)), "{body}");
    }
}

/// R6: nothing usable is `None`; the scope still comes from the header.
#[test]
fn no_retry_after() {
    let truncated = {
        let mut body = br#"{"retry_after":1,"pad":""#.to_vec();
        body.resize(64 * 1024, b'x');
        body
    };
    for value in [None, Some("abc"), Some("-3"), Some("NaN")] {
        let mut map = headers(&[(SCOPE, "user")]);
        if let Some(value) = value {
            map.insert(header::RETRY_AFTER, HeaderValue::from_static(value));
        }
        for body in [&b"not json"[..], b"", &truncated] {
            let limited = classify_429(&map, body);
            assert_eq!(limited.retry_after, None, "{value:?}");
            assert_eq!(limited.scope, RateLimitScope::User);
        }
    }
}

/// R7.
#[test]
fn discord_code() {
    for (raw, expected) in [("20028", Some(20028)), ("4294967296", None), ("-1", None)] {
        let body = format!(r#"{{"code":{raw}}}"#);
        assert_eq!(
            classify_429(&HeaderMap::new(), body.as_bytes()).discord_code,
            expected,
            "{raw}"
        );
    }
}

const VALID: [(&str, &str); 5] = [
    (SCOPE, "user"),
    (BUCKET, "b"),
    (LIMIT, "5"),
    (REMAINING, "0"),
    (RESET_AFTER, "0.5"),
];

fn valid_with(name: &str, value: Option<&'static str>) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (n, v) in VALID {
        if n == name {
            if let Some(value) = value {
                map.insert(n, HeaderValue::from_static(value));
            }
        } else {
            map.insert(n, HeaderValue::from_static(v));
        }
    }
    map
}

/// R8 (RL-01): a direct-mode `user` 429 with valid bucket headers goes to
/// twilight's limiter.
#[test]
fn rl_01_direct_user_429_is_relayed() {
    assert!(relay_429(&headers(&VALID), false));
}

/// R9: never behind a proxy.
#[test]
fn proxied_never_relays() {
    assert!(!relay_429(&headers(&VALID), true));
}

/// R10 (RL-04): a `shared`, `global` or scope-less 429 is surfaced.
#[test]
fn rl_04_shared_429_is_surfaced() {
    for value in [Some("shared"), Some("global"), None] {
        assert!(!relay_429(&valid_with(SCOPE, value), false), "{value:?}");
    }
}

/// R11.
#[test]
fn missing_bucket_header_surfaces() {
    for name in [BUCKET, LIMIT, REMAINING, RESET_AFTER] {
        assert!(!relay_429(&valid_with(name, None), false), "{name}");
    }
}

/// R12, R13.
#[test]
fn invalid_bucket_values_surface() {
    for (name, value) in [
        (LIMIT, "65536"),
        (REMAINING, "-1"),
        (LIMIT, " 5"),
        (RESET_AFTER, "-0.1"),
        (RESET_AFTER, "NaN"),
        (RESET_AFTER, "inf"),
        (RESET_AFTER, "86400.5"),
        (RESET_AFTER, "abc"),
    ] {
        assert!(
            !relay_429(&valid_with(name, Some(value)), false),
            "{name}: {value}"
        );
    }
    for value in ["0", "86400"] {
        assert!(
            relay_429(&valid_with(RESET_AFTER, Some(value)), false),
            "{value}"
        );
    }
}

fn all_values() -> Vec<RateLimited> {
    let mut out = Vec::new();
    for scope in [
        RateLimitScope::User,
        RateLimitScope::Global,
        RateLimitScope::Shared,
        RateLimitScope::Unknown,
    ] {
        for retry_after in [
            None,
            Some(Duration::from_millis(1500)),
            Some(Duration::ZERO),
        ] {
            for discord_code in [None, Some(20028)] {
                out.push(RateLimited {
                    scope,
                    retry_after,
                    discord_code,
                });
            }
        }
    }
    out
}

/// R14: the edge body round-trips, starts with the prefix, has no `code`
/// and parses as twilight's `ApiError::Ratelimited` with a wait.
#[test]
fn edge_body_round_trip() {
    for limited in all_values() {
        let body = edge_body(&limited);
        assert!(body.starts_with(RATE_LIMIT_BODY_PREFIX.as_bytes()));
        assert!(body.starts_with(crate::edge::EDGE_BODY_PREFIX.as_bytes()));
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("code").is_none(), "{json}");
        assert_eq!(
            json["global"],
            Value::Bool(limited.scope == RateLimitScope::Global)
        );
        assert_eq!(
            parse_edge_body(503, &body).as_ref(),
            Some(&limited),
            "{json}"
        );
        let api: twilight_http::api_error::ApiError = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            matches!(api, twilight_http::api_error::ApiError::Ratelimited(_)),
            limited.retry_after.is_some(),
            "{json}"
        );
    }
}

/// R15.
#[test]
fn parse_edge_body_rejects() {
    let body = edge_body(&RateLimited::new(RateLimitScope::Shared));
    assert_eq!(parse_edge_body(429, &body), None);
    assert_eq!(parse_edge_body(502, &body), None);
    assert_eq!(
        parse_edge_body(
            503,
            br#"{"message":"You are being rate limited.","retry_after":1.0,"global":false}"#
        ),
        None
    );
    let mut garbage = RATE_LIMIT_BODY_PREFIX.as_bytes().to_vec();
    garbage.extend_from_slice(b",nope");
    assert_eq!(parse_edge_body(503, &garbage), None);
    assert_eq!(parse_edge_body(503, b""), None);
}

/// R18.
#[test]
fn display_and_names() {
    let mut limited = RateLimited::new(RateLimitScope::Shared);
    assert_eq!(
        limited.to_string(),
        "Discord rate limited the request (shared scope)"
    );
    limited.retry_after = Some(Duration::from_millis(1500));
    assert_eq!(
        limited.to_string(),
        "Discord rate limited the request (shared scope), retry after 1.500 s"
    );
    let names: Vec<_> = [
        RateLimitScope::User,
        RateLimitScope::Global,
        RateLimitScope::Shared,
        RateLimitScope::Unknown,
    ]
    .iter()
    .map(|s| s.as_str())
    .collect();
    assert_eq!(names, ["user", "global", "shared", "unknown"]);
}
