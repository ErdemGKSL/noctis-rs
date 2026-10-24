//! The `AssetFetcher` against a loopback HTTP/1.1 server (IMG-03): fetch,
//! caps, statuses, redirects, timeouts, bodies, the allow-list, the cache,
//! concurrency and cancellation.

use std::sync::Mutex;
use std::time::{Duration, Instant as StdInstant};

use noctis_ports::PortError;
use noctis_ports::memory::MemoryAssetStore;

use super::*;
use crate::remote::test_server::{Reply, Server, capture_lines, png};

const RED: [u8; 4] = [255, 0, 0, 255];

/// A fetcher allowing `server` (and building CDN URLs on it).
fn fetcher_for(server: &Server, config: RemoteConfig) -> AssetFetcher {
    AssetFetcher::new(&config.discord_cdn(server.origin())).unwrap()
}

fn fetcher(server: &Server) -> AssetFetcher {
    fetcher_for(server, RemoteConfig::new())
}

fn image(server: &Server, path: &str) -> RemoteImage {
    RemoteImage::url(&server.url(path)).unwrap()
}

/// The reason one image fails.
async fn reason(fetcher: &AssetFetcher, image: &RemoteImage) -> String {
    fetcher.fetch(None, "t", image, false).await.unwrap_err()
}

fn declared(field: &'static str, required: bool, image: RemoteImage) -> Declared {
    Declared {
        field: field.into(),
        required,
        image,
    }
}

#[tokio::test]
async fn img_03_fetches_and_decodes_a_png() {
    let server = Server::start(|_| Reply::Ok(png(2, 2, RED))).await;
    let fetcher = fetcher(&server);
    let source = fetcher
        .fetch(None, "t", &image(&server, "/a.png?x=1"), false)
        .await
        .unwrap();
    assert!(matches!(source, ImageSource::Bitmap(_)), "{source:?}");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].target, "/a.png?x=1");
    let head = &requests[0].head;
    assert!(head.starts_with("get /a.png?x=1 http/1.1\r\n"), "{head}");
    assert!(
        head.contains(&format!(
            "user-agent: noctis/{}\r\n",
            env!("CARGO_PKG_VERSION")
        )),
        "{head}"
    );
    assert!(!head.contains("accept-encoding"), "{head}");
    assert!(!head.contains("cookie"), "{head}");
}

#[tokio::test]
async fn img_03_byte_cap() {
    let exact = png(2, 2, RED);
    let cap = 1024_u64;
    let padded = {
        // A PNG padded to exactly the cap with trailing bytes.
        let mut body = exact.clone();
        body.resize(usize::try_from(cap).unwrap(), 0);
        body
    };
    let over = {
        let mut body = padded.clone();
        body.push(0);
        body
    };
    let (ok_body, over_body) = (padded.clone(), over.clone());
    let server = Server::start(move |target| match target {
        "/exact" => Reply::Ok(ok_body.clone()),
        "/over" => Reply::Unsized(over_body.clone()),
        "/over-sized" => Reply::Ok(over.clone()),
        _ => Reply::Claim(cap + 1),
    })
    .await;
    let fetcher = fetcher_for(&server, RemoteConfig::new().max_bytes(cap));
    assert!(
        fetcher
            .fetch(None, "t", &image(&server, "/exact"), false)
            .await
            .is_ok()
    );
    let message = "over the byte cap of 1024 bytes";
    assert_eq!(reason(&fetcher, &image(&server, "/over")).await, message);
    assert_eq!(
        reason(&fetcher, &image(&server, "/over-sized")).await,
        message
    );
    // A `content-length` over the cap fails before the body is read: the
    // server never sends one, so only the header check can end this.
    let started = StdInstant::now();
    assert_eq!(reason(&fetcher, &image(&server, "/claim")).await, message);
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[tokio::test]
async fn img_03_statuses_and_redirects() {
    let second = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let location = second.url("/x.png");
    let server = Server::start(move |target| match target {
        "/404" => Reply::Status(404),
        "/500" => Reply::Status(500),
        _ => Reply::Redirect(location.clone()),
    })
    .await;
    let fetcher = fetcher_for(&server, RemoteConfig::new().allow_origin(second.origin()));
    assert_eq!(reason(&fetcher, &image(&server, "/404")).await, "HTTP 404");
    assert_eq!(reason(&fetcher, &image(&server, "/500")).await, "HTTP 500");
    assert_eq!(
        reason(&fetcher, &image(&server, "/301")).await,
        "redirect not followed"
    );
    assert_eq!(second.requests().len(), 0, "a redirect is never followed");
}

#[tokio::test]
async fn img_03_timeout() {
    let server = Server::start(|_| {
        Reply::After(Duration::from_secs(5), Box::new(Reply::Ok(png(1, 1, RED))))
    })
    .await;
    let fetcher = fetcher_for(
        &server,
        RemoteConfig::new().timeout(Duration::from_millis(100)),
    );
    let started = StdInstant::now();
    assert_eq!(
        reason(&fetcher, &image(&server, "/slow")).await,
        "timed out after 100ms"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn img_03_timeout_covers_body_and_permit() {
    // The head arrives at once; the body (10 bytes claimed) never does.
    let server = Server::start(|_| Reply::Claim(10)).await;
    let fetcher = fetcher_for(
        &server,
        RemoteConfig::new().timeout(Duration::from_millis(100)),
    );
    let started = StdInstant::now();
    assert_eq!(
        reason(&fetcher, &image(&server, "/body")).await,
        "timed out after 100ms"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    // With one permit, the second image times out waiting for it.
    let fetcher = fetcher_for(
        &server,
        RemoteConfig::new()
            .timeout(Duration::from_millis(100))
            .concurrency(1),
    );
    let (stalled, waiting) = (image(&server, "/stall"), image(&server, "/wait"));
    let started = StdInstant::now();
    let (first, second) = tokio::join!(reason(&fetcher, &stalled), reason(&fetcher, &waiting));
    assert_eq!(first, "timed out after 100ms");
    assert_eq!(second, "timed out after 100ms");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn img_03_bad_bodies() {
    let truncated = {
        let mut body = png(8, 8, RED);
        body.truncate(body.len() - 20);
        body
    };
    let big = png(3000, 2000, RED);
    let server = Server::start(move |target| match target {
        "/html" => Reply::Ok(b"<!doctype html><html></html>".to_vec()),
        "/gif" => Reply::Ok(b"GIF89a\x01\x00\x01\x00\x00\x00\x00".to_vec()),
        "/svg" => Reply::Ok(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec()),
        "/truncated" => Reply::Ok(truncated.clone()),
        _ => Reply::Ok(big.clone()),
    })
    .await;
    let fetcher = fetcher_for(&server, RemoteConfig::new().max_pixels(4_194_304));
    for path in ["/html", "/gif", "/svg"] {
        assert_eq!(
            reason(&fetcher, &image(&server, path)).await,
            "unsupported format: use PNG, JPEG or WebP",
            "{path}"
        );
    }
    let truncated = reason(&fetcher, &image(&server, "/truncated")).await;
    assert!(
        truncated.starts_with("could not be decoded: "),
        "{truncated}"
    );
    assert_eq!(
        reason(&fetcher, &image(&server, "/big")).await,
        "is 3000x2000: over 4194304 pixels"
    );
}

#[tokio::test]
async fn img_03_unreachable_names_the_io_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let fetcher = AssetFetcher::new(&RemoteConfig::new().allow_origin(origin.clone())).unwrap();
    let image = RemoteImage::url(&format!("{origin}/x.png")).unwrap();
    let reason = reason(&fetcher, &image).await;
    assert!(reason.to_ascii_lowercase().contains("connect"), "{reason}");
    assert!(reason.contains("refused"), "{reason}");
}

#[tokio::test]
async fn img_03_not_allow_listed_is_never_fetched() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let unlisted = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let fetcher = fetcher(&server);
    assert_eq!(
        reason(
            &fetcher,
            &RemoteImage::url("https://example.com/x.png").unwrap()
        )
        .await,
        "origin `https://example.com` is not allow-listed"
    );
    assert_eq!(
        reason(&fetcher, &image(&unlisted, "/x.png")).await,
        format!("origin `{}` is not allow-listed", unlisted.origin())
    );
    assert_eq!(unlisted.requests().len(), 0);
    assert_eq!(server.requests().len(), 0);
}

#[tokio::test]
async fn img_03_resolve_draws_the_fallback_or_fails_required() {
    let server = Server::start(|target| match target {
        "/ok" => Reply::Ok(png(1, 1, RED)),
        _ => Reply::Status(404),
    })
    .await;
    let fetcher = fetcher(&server);
    let (ok, missing) = (image(&server, "/ok"), image(&server, "/missing?token=1"));
    let resolved = fetcher
        .resolve(
            None,
            "card",
            vec![
                declared("a", false, ok.clone()),
                declared("b", false, missing.clone()),
            ],
            false,
        )
        .await
        .unwrap();
    assert!(resolved.get(&ok).is_some());
    assert!(
        resolved.get(&missing).is_some(),
        "a failed image draws the fallback"
    );
    assert!(resolved.degraded(), "FR-36: a fallback render is degraded");
    let fine = fetcher
        .resolve(None, "card", vec![declared("a", false, ok.clone())], false)
        .await
        .unwrap();
    assert!(!fine.degraded());
    let error = fetcher
        .resolve(None, "card", vec![declared("b", true, missing)], false)
        .await
        .unwrap_err();
    let RenderError::RemoteImage { id, image, reason } = error else {
        panic!("expected RemoteImage, got {error:?}");
    };
    assert_eq!(
        (id.as_str(), image, reason.as_str()),
        ("card", format!("{}/missing", server.origin()), "HTTP 404")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn img_03_each_fallback_logs_one_warning() {
    let (lines, _guard) = capture_lines();
    let server = Server::start(|_| Reply::Status(404)).await;
    let fetcher = fetcher(&server);
    let unlisted = RemoteImage::url("https://example.com/x.png").unwrap();
    let missing = image(&server, "/missing?token=1");
    fetcher
        .resolve(
            None,
            "card",
            vec![
                declared("a", false, unlisted),
                declared("b", false, missing),
            ],
            false,
        )
        .await
        .unwrap();
    let warnings = lines.warnings();
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    let unlisted_line = &warnings[0];
    assert!(unlisted_line.contains("field=a"), "{unlisted_line}");
    assert!(
        unlisted_line.contains("origin `https://example.com` is not allow-listed"),
        "{unlisted_line}"
    );
    assert!(
        unlisted_line.contains("image=https://example.com/x.png"),
        "{unlisted_line}"
    );
    let missing_line = &warnings[1];
    assert!(missing_line.contains("field=b"), "{missing_line}");
    assert!(missing_line.contains("HTTP 404"), "{missing_line}");
    assert!(
        missing_line.contains(&format!("image={}/missing", server.origin())),
        "{missing_line}"
    );
    for line in &warnings {
        assert!(!line.contains("token="), "{line}");
    }
    // The placeholder never fetches and never logs.
    let before = lines.warnings().len();
    let plan = fetcher
        .plan(
            "card",
            vec![declared("p", false, RemoteImage::placeholder())],
        )
        .unwrap();
    fetcher.resolve(None, "card", plan, false).await.unwrap();
    assert_eq!(lines.warnings().len(), before);
}

#[tokio::test]
async fn img_03_plan_dedupes_and_caps_the_count() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let fetcher = fetcher(&server);
    let a = image(&server, "/a");
    let plan = fetcher
        .plan(
            "card",
            vec![
                declared("a", false, a.clone()),
                declared("b", true, a.clone()),
                declared("p", true, RemoteImage::placeholder()),
            ],
        )
        .unwrap();
    assert_eq!(plan, [declared("a", true, a)]);
    let many: Vec<Declared> = (0..17)
        .map(|i| declared("v", false, image(&server, &format!("/{i}"))))
        .collect();
    assert!(matches!(
        fetcher.plan("card", many),
        Err(RenderError::OverCap {
            id,
            cap: RenderCap::RemoteImages,
            limit: 16,
            actual: 17,
        }) if id == "card"
    ));
    assert_eq!(server.requests().len(), 0);
}

/// An `AssetStore` that records its calls and can fail them.
#[derive(Default)]
struct Recording {
    inner: MemoryAssetStore,
    puts: Mutex<Vec<(String, Option<Duration>)>>,
    fail_get: bool,
    fail_put: bool,
}

impl AssetStore for Recording {
    fn get<'a>(
        &'a self,
        key: &'a str,
    ) -> noctis_ports::BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        if self.fail_get {
            return Box::pin(async { Err(PortError::Unavailable { why: "down".into() }) });
        }
        self.inner.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> noctis_ports::BoxFuture<'a, Result<(), PortError>> {
        self.puts.lock().unwrap().push((key.to_owned(), ttl));
        if self.fail_put {
            return Box::pin(async { Err(PortError::Unavailable { why: "down".into() }) });
        }
        self.inner.put(key, value, ttl)
    }
}

#[tokio::test]
async fn img_03_cache_hits_and_revalidation() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let fetcher = fetcher_for(
        &server,
        RemoteConfig::new().cache_ttl(Duration::from_mins(1)),
    );
    let store = Recording::default();
    let image = image(&server, "/a.png");
    let key = image.target(&fetcher.cdn, false).unwrap().unwrap().key;
    for _ in 0..2 {
        fetcher
            .fetch(Some(&store), "t", &image, false)
            .await
            .unwrap();
    }
    assert_eq!(
        server.requests().len(),
        1,
        "the second fetch is a cache hit"
    );
    assert_eq!(
        *store.puts.lock().unwrap(),
        [(key.clone(), Some(Duration::from_mins(1)))]
    );
    // Garbage in the store is a miss: refetched and rewritten.
    store
        .inner
        .put(&key, Bytes::from_static(b"not an image"), None)
        .await
        .unwrap();
    fetcher
        .fetch(Some(&store), "t", &image, false)
        .await
        .unwrap();
    assert_eq!(server.requests().len(), 2);
    assert_eq!(store.puts.lock().unwrap().len(), 2);
    // A failed fetch is never cached.
    let missing = Server::start(|_| Reply::Status(404)).await;
    let other = fetcher_for(&missing, RemoteConfig::new());
    let image = RemoteImage::url(&missing.url("/x")).unwrap();
    assert!(other.fetch(Some(&store), "t", &image, false).await.is_err());
    assert_eq!(store.puts.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn img_03_cache_port_errors_never_fail_the_fetch() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let fetcher = fetcher(&server);
    let image = image(&server, "/a.png");
    let failing = Recording {
        fail_get: true,
        fail_put: true,
        ..Recording::default()
    };
    for _ in 0..2 {
        fetcher
            .fetch(Some(&failing), "t", &image, false)
            .await
            .unwrap();
    }
    assert_eq!(server.requests().len(), 2);
    // Unbound: fetched every time.
    fetcher.fetch(None, "t", &image, false).await.unwrap();
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn img_03_concurrency_is_bounded_per_fetcher() {
    for (concurrency, expected) in [(1, 1), (2, 2)] {
        let server = Server::start(|_| {
            Reply::After(
                Duration::from_millis(100),
                Box::new(Reply::Ok(png(1, 1, RED))),
            )
        })
        .await;
        let fetcher = fetcher_for(&server, RemoteConfig::new().concurrency(concurrency));
        let plan = (0..3)
            .map(|i| declared("v", false, image(&server, &format!("/{i}"))))
            .collect();
        fetcher.resolve(None, "t", plan, false).await.unwrap();
        assert_eq!(server.requests().len(), 3);
        assert_eq!(
            server.max_in_flight(),
            expected,
            "concurrency {concurrency}"
        );
    }
}

#[tokio::test]
async fn img_03_dropping_the_resolve_aborts_the_fetch() {
    let server = Server::start(|_| {
        Reply::After(Duration::from_secs(30), Box::new(Reply::Ok(png(1, 1, RED))))
    })
    .await;
    let fetcher = fetcher(&server);
    let plan = vec![declared("a", false, image(&server, "/stall"))];
    let resolve = fetcher.resolve(None, "t", plan, false);
    let dropped = tokio::time::timeout(Duration::from_millis(200), resolve).await;
    assert!(dropped.is_err(), "the server stalls");
    let deadline = StdInstant::now() + Duration::from_secs(5);
    while server.aborted() == 0 {
        assert!(StdInstant::now() < deadline, "the fetch was not aborted");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn img_03_the_builtin_fallback_is_one_slate_pixel() {
    let ImageSource::Bitmap(buffer) = builtin_fallback().unwrap() else {
        panic!("expected a bitmap");
    };
    assert_eq!((buffer.width(), buffer.height()), (1, 1));
    assert_eq!(buffer.pixel(0, 0), [0x47, 0x55, 0x69, 0xFF]);
}

#[tokio::test(flavor = "current_thread")]
async fn img_04_a_decoded_emoji_id_zero_draws_the_fallback() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, [255, 0, 0, 255]))).await;
    let fetcher = fetcher(&server);
    let (lines, _guard) = capture_lines();
    let forged = RemoteImage {
        source: super::super::Source::Emoji {
            id: 0,
            animated: false,
        },
    };
    let resolved = fetcher
        .resolve(
            None,
            "card",
            vec![declared("text", false, forged.clone())],
            false,
        )
        .await
        .unwrap();
    assert!(resolved.get(&forged).is_some());
    assert_eq!(server.requests().len(), 0);
    let warnings = lines.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains("emoji id 0 is invalid"),
        "{}",
        warnings[0]
    );
    assert!(warnings[0].contains("image=emoji/0"), "{}", warnings[0]);
}

#[test]
fn img_04_plan_emoji_skips_declared_ones_and_caps_the_rest() {
    use std::num::NonZeroU64;

    use crate::emoji::CustomEmoji;

    let fetcher = AssetFetcher::new(&RemoteConfig::new().max_emoji(1)).unwrap();
    let e = |id: u64| CustomEmoji {
        id: NonZeroU64::new(id).unwrap(),
        animated: false,
    };
    let declared_emoji = RemoteImage::custom_emoji(Id::new(7), false);
    let mut plan = vec![declared("e", true, declared_emoji)];
    let over = fetcher.plan_emoji("card", &[e(7), e(8), e(9)], &mut plan);
    assert_eq!(plan.len(), 2);
    assert!(plan[0].required, "the declared field keeps its own entry");
    assert_eq!(plan[1].field, "text");
    assert!(!plan[1].required);
    assert_eq!(over, [RemoteImage::custom_emoji(Id::new(9), false)]);
}

#[test]
fn fr_36_emoji_over_the_cap_degrade_the_render() {
    let fallback = builtin_fallback().unwrap();
    let mut resolved = Resolved::empty(fallback);
    assert!(!resolved.degraded());
    resolved.fall_back(Vec::new());
    assert!(!resolved.degraded());
    resolved.fall_back(vec![RemoteImage::custom_emoji(Id::new(9), false)]);
    assert!(resolved.degraded());
}

#[test]
fn fr_36_key_pairs_follow_the_cdn_and_the_fallback() {
    let fallback: &'static [u8] = png(1, 1, RED).leak();
    let pairs = |config: RemoteConfig| AssetFetcher::new(&config).unwrap().key_pairs();
    let default = pairs(RemoteConfig::new());
    assert_eq!(default[0].0, "remote.cdn");
    assert_eq!(
        default[1],
        ("remote.fallback", *blake3::hash(b"builtin").as_bytes())
    );
    assert_eq!(default, pairs(RemoteConfig::new()));
    let cdn = pairs(RemoteConfig::new().discord_cdn("https://cdn.example.com"));
    assert_ne!(cdn[0], default[0]);
    assert_eq!(cdn[1], default[1]);
    let image = crate::StaticImage::new("fallback", fallback);
    let custom = pairs(RemoteConfig::new().fallback(image));
    assert_eq!(custom[0], default[0]);
    assert_eq!(custom[1].1, *blake3::hash(fallback).as_bytes());
}
