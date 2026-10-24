//! The render cache and single-flight through `TakumiRenderer::render`
//! (FR-36): concurrent and sequential identical requests, the digest,
//! admission, cancellation, shared errors, degraded renders, port failures
//! and the key's Theme and asset inputs.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_util::future::join_all;
use noctis_ports::memory::MemoryRenderCache;
use noctis_ports::{AssetStore, PortError, RenderCache, RendererPorts};

use super::*;
use crate::remote::test_server::{Reply, Server, capture_lines, png};
use crate::{AssetField, DeclaredImages, RemoteConfig, RemoteImage, StaticImage, TemplateAssets};

const RED: [u8; 4] = [255, 0, 0, 255];

/// Defines a 2×2 still Template `$name { tag: u8 }` whose view counts its
/// calls in `$count`, blocks on `$gate` while it is set, and panics with
/// `"boom"` for tag 255.
macro_rules! counted {
    ($name:ident, $id:literal, $count:ident, $gate:ident) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct $name {
            tag: u8,
        }

        static $count: AtomicUsize = AtomicUsize::new(0);
        static $gate: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);

        impl Template for $name {
            const ID: &'static str = $id;
            const VERSION: u32 = 1;
            const SIZE: Size = Size::new(2, 2);
            const FORMAT: Format = Format::Png;
            const ANIMATION: Option<AnimationSpec> = None;

            fn view() -> ErasedView {
                erased_view::<Self>()
            }
        }

        impl crate::TemplateAssets for $name {}

        impl TemplateView for $name {
            fn view(&self, _: Frame) -> Node {
                $count.fetch_add(1, Ordering::SeqCst);
                if let Some(gate) = $gate.lock().unwrap().as_ref() {
                    gate.recv_timeout(Duration::from_secs(10)).unwrap();
                }
                assert!(self.tag != 255, "boom");
                div(Classes::__literal("w-full h-full bg-brand"))
            }
        }
    };
}

counted!(Conc, "conc", CONC_VIEWS, CONC_GATE);
counted!(Distinct, "distinct", DISTINCT_VIEWS, DISTINCT_GATE);
counted!(Seq, "seq", SEQ_VIEWS, SEQ_GATE);
counted!(SeqNo, "seq-no", SEQ_NO_VIEWS, SEQ_NO_GATE);
counted!(Adm, "adm", ADM_VIEWS, ADM_GATE);
counted!(Lead, "lead", LEAD_VIEWS, LEAD_GATE);
counted!(AllDrop, "all-drop", ALL_DROP_VIEWS, ALL_DROP_GATE);
counted!(Boomy, "boomy", BOOMY_VIEWS, BOOMY_GATE);
counted!(GetFails, "get-fails", GET_FAILS_VIEWS, GET_FAILS_GATE);
counted!(PutFails, "put-fails", PUT_FAILS_VIEWS, PUT_FAILS_GATE);
counted!(Corrupt, "corrupt", CORRUPT_VIEWS, CORRUPT_GATE);
counted!(Pends, "pends", PENDS_VIEWS, PENDS_GATE);

/// A still Template declaring and drawing `images`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Avatar {
    images: Vec<RemoteImage>,
    required: bool,
}

impl Template for Avatar {
    const ID: &'static str = "avatar";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(4, 4);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl TemplateAssets for Avatar {
    const DECLARES: bool = true;

    fn declare(&self, out: &mut DeclaredImages) {
        AssetField::declare(&self.images, "images", self.required, out);
    }
}

impl TemplateView for Avatar {
    fn view(&self, _: Frame) -> Node {
        self.images.iter().fold(
            div(Classes::__literal("flex w-full h-full")),
            |root, image| root.child(crate::img(image, Classes::__literal("w-full h-full"))),
        )
    }
}

/// What a test Renderer differs in.
struct Setup {
    threads: usize,
    capacity: usize,
    theme: &'static str,
    images: Vec<StaticImage>,
    fonts: Vec<Font>,
    remote: RemoteConfig,
}

impl Setup {
    fn new() -> Self {
        Self {
            threads: 2,
            capacity: 64,
            theme: THEME,
            images: Vec::new(),
            fonts: vec![Font::from_static(INTER)],
            remote: RemoteConfig::new(),
        }
    }

    fn build(self) -> TakumiRenderer {
        let pool = Runtime::for_test()
            .resource_or_init::<RenderPool>(PoolConfig {
                threads: self.threads,
                queue_capacity: self.capacity,
            })
            .unwrap();
        let theme = Theme::parse(self.theme).unwrap();
        let stylesheet = theme_stylesheet(&theme).unwrap();
        let renderer = TakumiRenderer::new(RendererConfig {
            pool,
            fonts: test_fonts(self.fonts),
            images: StaticImages::decode(&self.images).unwrap(),
            theme,
            stylesheet,
            mode: ClassCheckMode::Deny,
            size_target: crate::plugin::DEFAULT_SIZE_TARGET,
            still_fallback: true,
            caps: RenderCaps::new(),
            remote: AssetFetcher::new(&self.remote).unwrap(),
        });
        renderer
            .register(&[
                TemplateDescriptor::of::<Conc>(),
                TemplateDescriptor::of::<Distinct>(),
                TemplateDescriptor::of::<Seq>(),
                TemplateDescriptor::of::<SeqNo>(),
                TemplateDescriptor::of::<Adm>(),
                TemplateDescriptor::of::<Lead>(),
                TemplateDescriptor::of::<AllDrop>(),
                TemplateDescriptor::of::<Boomy>(),
                TemplateDescriptor::of::<GetFails>(),
                TemplateDescriptor::of::<PutFails>(),
                TemplateDescriptor::of::<Corrupt>(),
                TemplateDescriptor::of::<Pends>(),
                TemplateDescriptor::of::<Avatar>(),
                TemplateDescriptor::of::<Card>(),
            ])
            .unwrap();
        renderer
    }
}

/// A `RenderCache` that counts its calls, over a `MemoryRenderCache`; it
/// can fail `get` or `put`, or make `get` pend once.
#[derive(Default)]
struct TestCache {
    inner: MemoryRenderCache,
    gets: AtomicUsize,
    puts: AtomicUsize,
    fail_get: bool,
    fail_put: bool,
    pend_get: bool,
}

/// Pending once (waking itself), then ready.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn down() -> PortError {
    PortError::Backend("down".into())
}

impl RenderCache for TestCache {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.pend_get {
                YieldOnce(false).await;
            }
            if self.fail_get {
                return Err(down());
            }
            self.inner.get(key).await
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        if self.fail_put {
            return Box::pin(async { Err(down()) });
        }
        self.inner.put(key, value, ttl)
    }
}

impl TestCache {
    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }

    fn puts(&self) -> usize {
        self.puts.load(Ordering::SeqCst)
    }
}

/// Binds `cache` (and a fresh asset store) to `r`.
fn bind(r: &TakumiRenderer, cache: &Arc<TestCache>) {
    let store: Arc<dyn AssetStore> = Arc::new(noctis_ports::memory::MemoryAssetStore::new());
    r.bind(RendererPorts::new(store).with_render_cache(Arc::clone(cache) as Arc<dyn RenderCache>));
}

/// Binds a counting `TestCache` to `r`.
fn bound(r: &TakumiRenderer) -> Arc<TestCache> {
    let cache = Arc::new(TestCache::default());
    bind(r, &cache);
    cache
}

/// The render cache warnings among `lines`.
fn cache_warnings(lines: &crate::remote::test_server::Lines) -> Vec<String> {
    let mut warnings = lines.warnings();
    warnings.retain(|line| line.contains("render cache"));
    warnings
}

fn job<T: Template>(template: &T) -> RenderJob {
    RenderJob::of(template).unwrap()
}

#[tokio::test]
async fn fr_36_concurrent_identical_requests_render_once() {
    let r = Setup::new().build();
    let (open, gate) = mpsc::channel();
    *CONC_GATE.lock().unwrap() = Some(gate);
    let futures: Vec<_> = (0..8).map(|_| r.render(job(&Conc { tag: 1 }))).collect();
    assert_eq!(r.pool.admitted(), 1, "one queue slot");
    assert_eq!(r.shared.flights.len(), 1);
    open.send(()).unwrap();
    let outputs: Vec<RenderOutput> = join_all(futures)
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect();
    *CONC_GATE.lock().unwrap() = None;
    assert_eq!(CONC_VIEWS.load(Ordering::SeqCst), 1);
    assert!(outputs.iter().all(|out| *out == outputs[0]));
    assert_eq!(r.shared.flights.len(), 0);
}

#[tokio::test]
async fn fr_36_distinct_requests_render_apart() {
    let r = Setup::new().build();
    let (a, b) = tokio::join!(
        r.render(job(&Distinct { tag: 1 })),
        r.render(job(&Distinct { tag: 2 }))
    );
    assert_eq!(DISTINCT_VIEWS.load(Ordering::SeqCst), 2);
    assert_ne!(a.unwrap().digest, b.unwrap().digest);
}

#[tokio::test]
async fn fr_36_a_bound_cache_serves_the_second_request() {
    let r = Setup::new().build();
    let cache = bound(&r);
    let first = r.render(job(&Seq { tag: 1 })).await.unwrap();
    let second = r.render(job(&Seq { tag: 1 })).await.unwrap();
    assert_eq!(
        SEQ_VIEWS.load(Ordering::SeqCst),
        1,
        "the hit renders nothing"
    );
    assert_eq!(second, first);
    assert_eq!((cache.gets(), cache.puts()), (2, 1));
    let entry = cache
        .inner
        .get(&RenderKey(first.digest).store_key())
        .await
        .unwrap();
    assert!(entry.is_some());
}

#[tokio::test]
async fn fr_36_without_a_cache_every_request_renders() {
    let r = Setup::new().build();
    let first = r.render(job(&SeqNo { tag: 1 })).await.unwrap();
    let second = r.render(job(&SeqNo { tag: 1 })).await.unwrap();
    assert_eq!(SEQ_NO_VIEWS.load(Ordering::SeqCst), 2);
    assert_eq!(first.digest, second.digest);
}

#[tokio::test]
async fn fr_36_the_digest_is_the_render_key() {
    let r = Setup::new().build();
    let card = card("Ada");
    let key = r.key(&card);
    let out = r.render(card).await.unwrap();
    assert_eq!(out.digest, key.0);
    assert_ne!(out.digest, *blake3::hash(&out.bytes).as_bytes());
}

#[tokio::test]
async fn fr_36_a_memory_cache_keeps_admission_at_the_call() {
    let r = Setup {
        threads: 1,
        capacity: 2,
        ..Setup::new()
    }
    .build();
    let _cache = bound(&r);
    let (open, gate) = mpsc::channel();
    *ADM_GATE.lock().unwrap() = Some(gate);
    let a = r.render(job(&Adm { tag: 0 }));
    let b = r.render(job(&Adm { tag: 1 }));
    assert!(matches!(
        r.render(job(&Adm { tag: 2 })).await,
        Err(RenderError::QueueFull)
    ));
    open.send(()).unwrap();
    open.send(()).unwrap();
    assert!(a.await.is_ok());
    assert!(b.await.is_ok());
    *ADM_GATE.lock().unwrap() = None;
}

#[tokio::test]
async fn fr_36_the_leader_dropped_leaves_the_joiner_its_render() {
    let r = Setup::new().build();
    let (open, gate) = mpsc::channel();
    *LEAD_GATE.lock().unwrap() = Some(gate);
    let a = r.render(job(&Lead { tag: 1 }));
    let b = r.render(job(&Lead { tag: 1 }));
    drop(a);
    open.send(()).unwrap();
    assert!(b.await.is_ok());
    *LEAD_GATE.lock().unwrap() = None;
    assert_eq!(LEAD_VIEWS.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn fr_36_every_caller_dropped_skips_the_job() {
    let r = Setup {
        threads: 1,
        ..Setup::new()
    }
    .build();
    let (open, gate) = mpsc::channel();
    *ALL_DROP_GATE.lock().unwrap() = Some(gate);
    let blocker = r.render(job(&AllDrop { tag: 0 }));
    // Wait until the blocker holds the only pool thread.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while ALL_DROP_VIEWS.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let a = r.render(job(&AllDrop { tag: 1 }));
    let b = r.render(job(&AllDrop { tag: 1 }));
    assert_eq!(r.shared.flights.len(), 2);
    drop(a);
    drop(b);
    assert_eq!(r.shared.flights.len(), 1, "the dropped flight left the map");
    open.send(()).unwrap();
    blocker.await.unwrap();
    *ALL_DROP_GATE.lock().unwrap() = None;
    drained(&r).await;
    assert_eq!(ALL_DROP_VIEWS.load(Ordering::SeqCst), 1, "tag 1 never ran");
    assert_eq!(r.shared.flights.len(), 0);
}

#[tokio::test]
async fn fr_36_an_error_is_shared_and_never_cached() {
    let r = Setup::new().build();
    let cache = bound(&r);
    let (open, gate) = mpsc::channel();
    *BOOMY_GATE.lock().unwrap() = Some(gate);
    let futures: Vec<_> = (0..3).map(|_| r.render(job(&Boomy { tag: 255 }))).collect();
    open.send(()).unwrap();
    for result in join_all(futures).await {
        assert!(
            matches!(&result, Err(RenderError::Panicked { message }) if message == "boom"),
            "{result:?}"
        );
    }
    assert_eq!(BOOMY_VIEWS.load(Ordering::SeqCst), 1);
    assert_eq!(cache.puts(), 0);
    *BOOMY_GATE.lock().unwrap() = None;
    assert!(r.render(job(&Boomy { tag: 255 })).await.is_err());
    assert_eq!(BOOMY_VIEWS.load(Ordering::SeqCst), 2, "not cached");
}

#[tokio::test]
async fn fr_36_synchronous_errors_touch_neither_cache_nor_map() {
    let r = Setup {
        remote: RemoteConfig::new().max_per_job(1),
        ..Setup::new()
    }
    .build();
    let cache = bound(&r);
    let unknown = RenderJob {
        template_version: 9,
        ..card("Ada")
    };
    let fps = RenderJob {
        animation: Some(AnimationSpec::new(
            Duration::from_secs(1),
            120,
            AnimatedFormat::Webp { quality: 80 },
        )),
        ..card("Ada")
    };
    let a = RemoteImage::url("https://cdn.discordapp.com/a.png").unwrap();
    let b = RemoteImage::url("https://cdn.discordapp.com/b.png").unwrap();
    let over = job(&Avatar {
        images: vec![a, b],
        required: false,
    });
    for job in [unknown, fps, over] {
        let future = r.render(job);
        assert_eq!(r.shared.flights.len(), 0);
        assert!(future.await.is_err());
    }
    assert_eq!((cache.gets(), cache.puts()), (0, 0));
}

#[tokio::test]
async fn fr_36_a_fallback_render_is_not_cached() {
    let server = Server::start(|_| Reply::Status(404)).await;
    let r = Setup {
        remote: RemoteConfig::new().discord_cdn(server.origin()),
        ..Setup::new()
    }
    .build();
    let cache = bound(&r);
    let a = RemoteImage::url(&server.url("/a.png")).unwrap();
    let avatar = |required| {
        job(&Avatar {
            images: vec![a.clone()],
            required,
        })
    };
    r.render(avatar(false)).await.unwrap();
    r.render(avatar(false)).await.unwrap();
    assert_eq!(server.requests().len(), 2, "rendered again");
    assert_eq!(cache.puts(), 0);
    let err = r.render(avatar(true)).await.unwrap_err();
    assert!(matches!(err, RenderError::RemoteImage { .. }), "{err}");
    assert_eq!(cache.puts(), 0);
}

#[tokio::test]
async fn fr_36_emoji_over_the_cap_are_not_cached() {
    let r = Setup {
        remote: RemoteConfig::new().max_emoji(0),
        ..Setup::new()
    }
    .build();
    let cache = bound(&r);
    r.render(card("hi <:wave:123456789012345678>"))
        .await
        .unwrap();
    r.render(card("hi <:wave:123456789012345678>"))
        .await
        .unwrap();
    assert_eq!((cache.gets(), cache.puts()), (2, 0));
}

#[tokio::test]
async fn fr_36_a_cache_hit_skips_the_fetch() {
    let server = Server::start(|_| Reply::Ok(png(2, 2, RED))).await;
    let r = Setup {
        remote: RemoteConfig::new().discord_cdn(server.origin()),
        ..Setup::new()
    }
    .build();
    let cache = bound(&r);
    let a = RemoteImage::url(&server.url("/a.png")).unwrap();
    let avatar = || {
        job(&Avatar {
            images: vec![a.clone()],
            required: false,
        })
    };
    let first = r.render(avatar()).await.unwrap();
    let second = r.render(avatar()).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(server.requests().len(), 1);
    assert_eq!(cache.puts(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn fr_36_a_failed_cache_read_renders_warns_and_writes() {
    let (lines, _guard) = capture_lines();
    let r = Setup::new().build();
    let cache = Arc::new(TestCache {
        fail_get: true,
        ..TestCache::default()
    });
    bind(&r, &cache);
    assert!(r.render(job(&GetFails { tag: 1 })).await.is_ok());
    assert_eq!(GET_FAILS_VIEWS.load(Ordering::SeqCst), 1);
    assert_eq!(cache.puts(), 1, "the put is still attempted");
    let warnings = cache_warnings(&lines);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("render cache read failed"),
        "{warnings:?}"
    );
    assert!(
        warnings[0].contains("template=\"get-fails\""),
        "{warnings:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fr_36_a_failed_cache_write_still_returns_the_output() {
    let (lines, _guard) = capture_lines();
    let r = Setup::new().build();
    let cache = Arc::new(TestCache {
        fail_put: true,
        ..TestCache::default()
    });
    bind(&r, &cache);
    assert!(r.render(job(&PutFails { tag: 1 })).await.is_ok());
    let warnings = cache_warnings(&lines);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("render cache write failed"),
        "{warnings:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fr_36_a_corrupt_entry_is_rendered_again_and_overwritten() {
    let (lines, _guard) = capture_lines();
    let r = Setup::new().build();
    let cache = bound(&r);
    let corrupt = job(&Corrupt { tag: 1 });
    let store_key = r.key(&corrupt).store_key();
    cache
        .inner
        .put(&store_key, Bytes::from_static(b"junk"), None)
        .await
        .unwrap();
    let out = r.render(corrupt).await.unwrap();
    assert_eq!(CORRUPT_VIEWS.load(Ordering::SeqCst), 1);
    let entry = cache.inner.get(&store_key).await.unwrap().unwrap();
    assert_eq!(
        crate::cache::decode_entry(&entry, RenderKey(out.digest)),
        Some(out)
    );
    let warnings = cache_warnings(&lines);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("not valid"), "{warnings:?}");
}

#[tokio::test]
async fn fr_36_a_pending_cache_admits_after_the_lookup() {
    let r = Setup::new().build();
    let cache = Arc::new(TestCache {
        pend_get: true,
        ..TestCache::default()
    });
    bind(&r, &cache);
    let future = r.render(job(&Pends { tag: 1 }));
    assert_eq!(r.pool.admitted(), 0, "not admitted before the lookup");
    let out = future.await.unwrap();
    assert_eq!(out.settings_used.size, Size::new(2, 2));
    assert_eq!(PENDS_VIEWS.load(Ordering::SeqCst), 1);
    assert_eq!(cache.puts(), 1);
}

#[tokio::test]
async fn fr_36_theme_and_asset_changes_miss_a_shared_cache() {
    let cache = Arc::new(TestCache::default());
    let render = async |setup: Setup| {
        let r = setup.build();
        bind(&r, &cache);
        r.render(card("Ada")).await.unwrap();
    };
    render(Setup::new()).await;
    assert_eq!(cache.puts(), 1);
    render(Setup::new()).await;
    assert_eq!(cache.puts(), 1, "an identical Renderer hits");
    let logo: &'static [u8] = png(1, 1, RED).leak();
    let fallback: &'static [u8] = png(1, 1, [0, 0, 255, 255]).leak();
    let variants = [
        Setup {
            theme: "@theme { --color-brand: #000000; }",
            ..Setup::new()
        },
        Setup {
            images: vec![StaticImage::new("logo", logo)],
            ..Setup::new()
        },
        Setup {
            fonts: vec![Font::from_static(INTER).family("Other")],
            ..Setup::new()
        },
        Setup {
            remote: RemoteConfig::new().fallback(StaticImage::new("fallback", fallback)),
            ..Setup::new()
        },
    ];
    for (i, setup) in variants.into_iter().enumerate() {
        render(setup).await;
        assert_eq!(cache.puts(), i + 2, "variant {i} misses");
    }
}
