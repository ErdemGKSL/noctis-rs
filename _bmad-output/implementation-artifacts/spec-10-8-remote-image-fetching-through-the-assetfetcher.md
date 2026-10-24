---
title: 'Story 10.8: Remote image fetching through the AssetFetcher'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
baseline_revision: '3f08cf6f3aa8c7179751a83947c937ea4f6ee265'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-7-fonts-and-static-assets.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-67 is not implemented. `#[derive(Template)]` rejects `#[asset]` ("planned (Story 10.8)"), `<img>` takes only a `StaticImage`, and there is no `RemoteImage`, no allow-list and no HTTP client. So a Template cannot show an avatar. The Project Template and the Usage Skill already use `#[asset] avatar: RemoteImage` and `RemoteImage::avatar(&user, 256)`, but that API does not exist yet.

**Approach:**
- **Declaring images.** A Template declares its remote images as `#[asset]` / `#[asset(required)]` fields. A field is a `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>`.
- **Resolving before the render.** Before the job enters the render pool, the takumi Renderer resolves every declared image through a per-App `AssetFetcher`:
  - an origin allow-list (the Discord CDN by default);
  - a timeout, a byte cap and a pixel cap;
  - a concurrency semaphore;
  - decoding on tokio's blocking pool;
  - caching of the fetched bytes through the App's namespaced `AssetStore`, which core hands to the Renderer with a new provided port method `Renderer::bind(RendererPorts)`.
- **Failures.** A failed image draws the configurable fallback and logs a warning. If its field is `required`, the render fails with a typed error instead.
- **HTTP client.** The `AssetFetcher` uses `hyper-util`'s legacy `Client` with `hyper-rustls` 0.27.10. Both are already in `Cargo.lock`, so no new crate is added.
- **Enforcement.** A new `deps-check` rule, `asset-http`, keeps the `AssetFetcher` the only non-Discord HTTP in the workspace.

## Boundaries & Constraints

**Always:**
- **HTTP client (AC 1).** The client is `hyper_util::client::legacy::Client` over `hyper_rustls::HttpsConnector<HttpConnector>`.
  - Pin `hyper-rustls = { version = "0.27.10", default-features = false }` in `[workspace.dependencies]` (crates.io max stable 0.27.10, verified 2026-10-24).
  - `noctis-image` takes it optionally behind `render`: `hyper-rustls` with features `["http1", "http2", "ring", "tls12", "rustls-platform-verifier"]`; `hyper-util` (workspace 0.1.21) with features `["client-legacy", "http1", "http2", "tokio"]`; `hyper` with `["client", "http1", "http2"]`; `http-body-util` (for `Limited`); `rustls` and `rustls-platform-verifier` (workspace).
  - TLS config: `rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider())).with_safe_default_protocol_versions()?.with_platform_verifier()?` (`rustls_platform_verifier::BuilderVerifierExt`), with ALPN `h2` then `http/1.1`. It never depends on a process default provider. If the verifier is unavailable, the plugin reports the violation `[IMG-03] the TLS verifier is unavailable: <error>`.
  - Connector: `HttpsConnectorBuilder::new().with_tls_config(cfg).https_or_http().enable_http1().enable_http2().build()`. Plain http is reachable only because loopback http origins may be allow-listed (see Origins).
  - Requests are `GET` with header `user-agent: noctis/<CARGO_PKG_VERSION>` and no `accept-encoding`. Nothing personal goes in any header.
  - The crate choice is recorded in the Design Notes and the memlog.
- **Never follow redirects.** Any 3xx is a failure (`redirect not followed`), so the allow-list cannot be escaped.
- **Origins.** An origin is `scheme://host[:port]`, optionally followed by one `/`.
  - Scheme is `https`, or `http` only when the host is `127.0.0.1`, `[::1]` or `localhost`. Host is lowercase ASCII `[a-z0-9.-]+` or an IP literal. No userinfo, path, query, fragment or wildcard.
  - Matching compares scheme, the lowercased host and the effective port (https 443, http 80) exactly.
  - The default allow-list is `https://cdn.discordapp.com` and `https://media.discordapp.net`.
  - `RemoteConfig::discord_cdn(origin)` (default `https://cdn.discordapp.com`) is the origin avatar and icon URLs are built on (a production override point, used by the tests). It is always allow-listed.
- **Per-App `AssetFetcher`.** One per App, owned by the Renderer (not a process Resource): the allow-list is per App and nothing conflicts. Its concurrency bound is one `tokio::sync::Semaphore`.
- **Timeout and caps.**
  - One permit is held for wait + connect + headers + body + decode.
  - `timeout` covers the permit wait, the request and the body (`tokio::time::timeout`), not the decode.
  - `content-length` over `max_bytes` fails before the body is read. The body is read through `http_body_util::Limited::new(body, max_bytes)`, so it never buffers more than the cap.
  - Format: sniffed with the existing `assets::sniff` (PNG/JPEG/WebP only; GIF, SVG, BMP and ICO are failures). The header's `w*h` must be at most `max_pixels`, checked before decoding.
  - Decoding runs `takumi::prelude::ImageSource::from_bytes` inside `tokio::task::spawn_blocking`. Animated WebP and APNG become takumi `ImageSource::Animated`, which takumi draws at each frame's `time_ms`.
- **Resolution order in `TakumiRenderer::render`.**
  1. `view_of` (the existing checks).
  2. If the Template's `ViewFn` declares assets: extract the declared images from `job.data`, dedupe them by source, and check the count against `max_per_job` (`RenderError::OverCap { cap: RenderCap::RemoteImages, .. }`, before any fetch).
  3. Resolve them all concurrently (`futures_util::future::join_all`, or a `JoinSet`).
  4. Only then `pool.try_submit`, with the per-job resolved map moved into the draw closure.

  Templates without `#[asset]` fields keep today's synchronous admission, byte for byte. Dropping the render future cancels pending fetches, and no job is submitted.
- **Cache through `AssetStore`.**
  - Keys: `img:v1:avatar:<user>:<hash>:<size>:<a|s>`, `img:v1:icon:<guild>:<hash>:<size>:<a|s>` and `img:v1:default-avatar:<index>`. A URL source is keyed `img:v1:url:<blake3 hex of its canonical URL>`.
  - The value is the fetched bytes, put with `Some(cache_ttl)` only after a successful sniff, pixel check and decode.
  - A hit is re-validated the same way; bytes that fail validation count as a miss and are refetched.
  - A `get`/`put` `PortError` logs a warning and never fails the render.
  - With no `bind` (a bare Renderer in unit tests), nothing is cached.
- **Failure handling.**
  - Every failed or blocked image logs once: `tracing::warn!(target: "noctis::image", template, field, image = %display, reason, "remote image unavailable; drawing the fallback")`.
  - The display never contains a URL query: `avatar/<user>/<hash>@<size>`, `icon/<guild>/<hash>@<size>`, `default-avatar/<i>`, `<scheme>://<host[:port]><path>`, `placeholder`.
  - Not required: draw the fallback. Required: `RenderError::RemoteImage { id, image, reason }`, reported to the Render sink like `Failed`. That needs no core change, because `fail` has wildcard arms; update its doc.
- **The fallback** is `RemoteConfig::fallback(StaticImage)`, decoded at install with the IMG-02 rules and checks. By default it is a built-in 1×1 opaque `#475569` PNG, encoded once at install with `image::codecs::png::PngEncoder`. `RemoteImage::placeholder()` always draws it, with no fetch and no log.
- **Untrusted Template data.** `RemoteImage` derives `Serialize`/`Deserialize`, so decoded data is untrusted: the allow-list, the URL grammar and size normalization are re-applied at resolve time, never only in constructors.
- **Memory (maintainer decision 5).** Fetched bytes are not secrets: no `Zeroizing`. `Debug` of `RemoteImage`/`RemoteConfig` prints the display form only.
- **Same commit (AGENTS.md).** Re-bless the API, update the Usage Skill, the template, `docs/discord-rules.md`, `noctis-rules` and `rules-catalog` evidence.

**Never:**
- No `reqwest`/`ureq`/other client crate. No new crate in `Cargo.lock`: it gains only dependency edges. Never run `cargo update`, and keep `takumi-core` at a single 0.31.0.
- No redirect following, no proxy support for asset fetches, no cookies, no `accept-encoding`.
- No takumi `gif`/`svg-source` features, so GIF avatars are never requested: Discord URLs use `.webp`, and `.png` for default avatars.
- No custom-emoji constructor or text parsing (Story 10.9). No render-cache key or single-flight change (Story 10.10). No cross-job fetch single-flight. No negative caching. No metrics (core owns render observability).
- No change to core's `ctx.render` beyond calling `Renderer::bind`.
- Never fetch an undeclared image.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| URL ok | `RemoteImage::url("https://cdn.discordapp.com/a/b.png?x=1")`; `HTTPS://CDN.DiscordApp.com/a` | `Ok`. Canonical form lowercases the scheme and host and keeps the path and query. | none |
| URL bounds | `""`; 2048 bytes; 2049 bytes | `Empty`; `Ok`; `TooLong { len: 2049 }` | `RemoteUrlError` (`#[non_exhaustive]`, thiserror) |
| URL bytes | `é` in the path; a space; `\u{7}` | `InvalidCharacter { index }` (the first byte outside 0x21..=0x7E) | same |
| URL shape | `ftp://x/y`; `file:///etc/passwd`; `data:image/png;base64,..`; `//h/x`; `cdn.discordapp.com/x` | `Scheme` | same |
| URL parts | `https://u:p@h/x`; `https://h/x#f`; `https:///x`; `https://:443/x`; `https://h:99999/x`; `https://h*/x` | `UserInfo`; `Fragment`; `NoHost`; `NoHost`; `Malformed`; `InvalidHost` | same |
| Size normalize | sizes 0, 15, 16, 17, 256, 257, 4096, 4097, `u16::MAX` | 16, 16, 16, 32, 256, 512, 4096, 4096, 4096 (the next power of two, clamped to 16..=4096) | none |
| Avatar | `User { id: 80351110224678912, avatar: Some(h), .. }`, size 256 | `<cdn>/avatars/80351110224678912/<h>.webp?size=256`, plus `&animated=true` only when `h.is_animated()` and the job is animated; key ends `:a` (animated request) or `:s` | none |
| Default avatar | `avatar: None` with discriminator 0, or with 1234 | `<cdn>/embed/avatars/<(id >> 22) % 6>.png`; index `1234 % 5 = 4` | none |
| Guild icon | `icon: Some(h)` at size 128; `icon: None` | `<cdn>/icons/<guild>/<h>.webp?size=128[&animated=true]`; `None` → `RemoteImage::placeholder()` | none |
| Origin config | `https://i.imgur.com`, `https://i.imgur.com/`, `https://h:8443`, `http://127.0.0.1:8080`, `http://[::1]:1`, `http://localhost:3000` | accepted; duplicates deduped | none |
| Origin invalid | `""`, `i.imgur.com`, `ftp://x`, `https://h/p`, `https://h?q`, `https://u@h`, `https://*.h`, `http://example.com` | `App::build` fails: one plugin violation each, `[IMG-03] remote origin `<s>` is invalid: <reason>` (http reason: "plain http is only for loopback hosts") | no Renderer |
| Config ranges | timeout 99 ms / 100 ms / 60 s / 61 s; `max_bytes` 1023 / 1024 / 67108864 / 67108865; concurrency 0 / 1 / 256 / 257; `max_per_job` 0 / 1 / 64 / 65; `max_pixels` 0 / 1 / 16777216 / 16777217; `cache_ttl` 0 / 1 s / 30 d / 30 d + 1 s | inside: OK; outside: `[IMG-03] remote <name> <v> is out of range: use <range>` | no Renderer |
| Defaults | `RemoteConfig::new()` | timeout 5 s, `max_bytes` 8 MiB (8388608), concurrency 8, `max_per_job` 16, `max_pixels` 4194304, `cache_ttl` 1 day, the built-in fallback, the default allow-list | none |
| Happy fetch | an allow-listed loopback server returns 200 + a 2×2 red PNG; `#[asset] a`, `<img src={&self.a} />` | the output pixel is red; exactly 1 request, made before the job is submitted | none |
| Byte cap | body of exactly `max_bytes` / `max_bytes + 1`; `content-length` over the cap | OK / failure "over the byte cap of N bytes" / failure before the body is read | fallback or `RemoteImage` |
| HTTP status | 404; 500; 301 with `location` pointing at a second server | failure "HTTP 404" / "HTTP 500" / "redirect not followed" (the second server gets 0 requests) | same |
| Timeout | the server sleeps past `timeout` | failure "timed out after 100ms" within about the timeout | same |
| Bad body | HTML; `GIF89a…`; `<svg`; a truncated PNG; a 3000×2000 PNG with `max_pixels` 4194304 | "unsupported format: use PNG, JPEG or WebP" / the same / the same / "could not be decoded: …" / "is 3000x2000: over 4194304 pixels" | same |
| Unreachable | connection refused | failure naming the I/O error | same |
| Not allow-listed | `RemoteImage::url("https://example.com/x.png")`; a loopback origin that is not listed | not fetched (0 requests); warning with reason "origin `https://example.com` is not allow-listed" | fallback, or `RemoteImage` when required |
| Port / scheme / host mismatch | `https://cdn.discordapp.com:444/x`; `http://cdn.discordapp.com/x`; `https://cdn.discordapp.com.evil.com/x` | not allow-listed; `https://cdn.discordapp.com:443/x` is allowed | as above |
| Required | `#[asset(required)]` with a 404 | `Err(RenderError::RemoteImage { id, image: "<display>", reason: "HTTP 404" })`, message `[IMG-03] template `<id>` requires remote image `<image>`, which could not be used: HTTP 404` | Render sink |
| Placeholder | `RemoteImage::placeholder()` in a required field | the fallback is drawn; no request, no log | none |
| Dedupe | the same image in two fields | 1 request; both nodes draw it | none |
| `Option` / `Vec` | `None`; `vec![]`; `vec![a, b]` | nothing declared / nothing / two declared (`field[0]`, `field[1]` in logs) | none |
| Count cap | 17 distinct images with the default `max_per_job` | `RenderError::OverCap { cap: RenderCap::RemoteImages, limit: 16, actual: 17 }`; 0 requests | before admission |
| Undeclared | a view draws `<img src={&r} />` for an `r` that no `#[asset]` field holds (not a placeholder) | `RenderError::UndeclaredImage { id, image }`: `[IMG-03] template `<id>` draws remote image `<image>`, which no `#[asset]` field declares` | Render sink |
| No asset fields | every existing Template | unchanged behavior; admission synchronous at call | none |
| Cache hit | render twice with a bound `MemoryAssetStore` | 1 request in total; the store holds `img:v1:…` with TTL `cache_ttl` | none |
| Cache garbage / port errors | the store returns non-image bytes; `get` → `Err`; `put` → `Err` | refetched; fetched with a warning; the render still succeeds with a warning | none |
| Unbound | `TakumiRenderer` without `bind` | it fetches on every render | none |
| Concurrency | `concurrency(1)`, 3 images, the server holds each request 100 ms | the server's max in-flight is 1; with `concurrency(2)` it is 2 | none |
| Cancel | drop the render future while the server is stalled | the fetch is aborted; the pool gets no job | none |
| Animated avatar | an animated Template (2 frames at 2 fps over 1 s) drawing an `a_` avatar; the server returns a 2-frame animated WebP (red 0–500 ms, blue 500–1000 ms) | the request has `animated=true`; output frame 0 is red, frame 1 blue. A still Template with the same avatar requests without `animated` | none |
| Derive ok | `#[asset] a: RemoteImage`; `#[asset(required)]`; `Option<RemoteImage>`; `Vec<RemoteImage>`; a tuple field (logged as `0`) | compiles | none |
| Derive errors | `#[asset] a: String`; `#[asset(optional)]`, `#[asset = 1]`, `#[asset(required, required)]`; two `#[asset]` on one field | the `on_unimplemented` error `[IMG-03] `#[asset]` fields are `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>``; `[IMG-03] `#[asset]` takes no argument or `required`: `#[asset(required)]``; `[IMG-03] `#[asset]` appears twice on this field` | compile error |
| `view!` img | `<img src={&self.avatar} />`; `<img src={LOGO} />`; `<img />` | `img(&self.avatar, ..)` and `img(LOGO, ..)` compile (`impl Into<ImageSrc>`); "`<img>` needs `src={..}` (a `StaticImage` or `RemoteImage`)" | compile error |
| deps-check | a member depends on `reqwest`; `noctis-core` depends on `hyper-rustls`; `noctis-image` has a non-optional `hyper-rustls`; `legacy::Client` in `noctis-image/src/plugin.rs` | `deps-check: … [asset-http]` violations; the real workspace passes | exit 1 |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs`:
  - `RenderCap` `:357-386`: add `RemoteImages` (`as_str` returns `"remote_images"`).
  - `RenderError` `:392`+: add `RemoteImage { id, image, reason: String }` and `UndeclaredImage { id, image }`.
  - `Renderer` trait: add a provided `fn bind(&self, ports: RendererPorts) {}` and a `#[non_exhaustive] pub struct RendererPorts { pub asset_store: Arc<dyn AssetStore> }` with `RendererPorts::new(asset_store)`.
  - Re-export it from `src/lib.rs` `:89`.
- `crates/noctis-core/src/app.rs` `:~2475`: right after `Ports::resolve`, call `renderer.bind(RendererPorts::new(ports.asset_store.clone()))` (the namespaced store).
- `crates/noctis-core/src/render.rs` `:163-170`: the `fail` doc names the two new variants (handled like `Failed`).
- `crates/noctis-image/src/template.rs`:
  - `ViewFn` gains `assets: Option<fn(&[u8]) -> Result<Vec<Declared>, RenderError>>`.
  - `erased_view::<T: Template + TemplateView + TemplateAssets>()` sets it from `T::DECLARES`.
  - The doctest adds `impl TemplateAssets for Card {}`.
  - Hand-written test Templates in `renderer/tests.rs` need the same one-line impl.
- `crates/noctis-image/src/node/mod.rs` `:68-72`, `:202-212`, `:259-262`:
  - `Kind::Image(ImageSrc)` with `pub enum ImageSrc { Static(StaticImage), Remote(RemoteImage) }` and `From` for `StaticImage`, `&StaticImage`, `RemoteImage`, `&RemoteImage`;
  - `Node::image(impl Into<ImageSrc>)` and `img(impl Into<ImageSrc>, Classes)`.
- `crates/noctis-image/src/lower.rs` `:30-53`: `lower(node, images, remote: &Resolved, template)`.
  - A static source is unchanged.
  - A remote source is looked up in the job's resolved map; placeholder → the fallback; missing → `UndeclaredImage`.
- `crates/noctis-image/src/renderer.rs`:
  - `Shared` `:44-58` gains `remote: AssetFetcher` and `store: OnceLock<Arc<dyn AssetStore>>`;
  - `RendererConfig` `:116`;
  - `render` `:216-232` gets the order in Boundaries;
  - `draw`/`frame` `:276-354` take the resolved map;
  - `bind` is implemented (first call wins).
- `crates/noctis-image/src/plugin.rs`:
  - `ImageConfig` `:93-101` gains `remote: RemoteConfig` and `.remote(RemoteConfig)`;
  - `check_config` `:216-273` adds the IMG-03 ranges and origins;
  - `install` builds the fetcher and decodes the fallback.
- `crates/noctis-image/src/assets.rs`: reuse `sniff`, the header dimensions and the decode helpers (make them `pub(crate)` as needed).
- `crates/noctis-macros/src/template.rs` `:127-134` (the planned `#[asset]` error), `emit` `:141-209`: parse `#[asset]`/`#[asset(required)]`. Always emit `impl ::noctis::__private::image::TemplateAssets for #ident`. With asset fields, `const DECLARES: bool = true;` and `fn declare(&self, out: &mut DeclaredImages) { AssetField::declare(&self.<f>, "<f>", <required>, out); … }`.
  - Tests in `template/tests.rs` `:338-345` change from "planned" to the new rows.
- `crates/noctis-macros/src/view.rs` `:210` (the `src` error text) and its tests `:560`+.
- `crates/noctis/src/lib.rs` `:1961-2100`: the image module docs, and re-exports of `RemoteImage`, `RemoteConfig`, `RemoteUrlError`, `ImageSrc`. Add `RendererPorts` next to `Renderer` (`:2088`).
- `xtask/src/deps_check.rs`:
  - the module doc `:1-31`;
  - constants `:56-70`;
  - `Workspace::from_metadata` `:276` keeps the deps named `hyper-rustls` and those in the banned list;
  - `check` `:459`, `Rule` `:385`;
  - tests `:989`+ (the `with_twilight_http` pattern).
- `crates/noctis-rules/src/rule.rs` `:78`: `IMG_03`. `docs/discord-rules.md` `:306-307`.
- Skill and template:
  - `examples/project-template/.claude/skills/noctis/references/images.md` §7 `:316-327` (rewrite it to the real API), `:87-88`;
  - `references/troubleshooting.md` `:99`, `references/limits-cheatsheet.md`, `SKILL.md` `:274` and the API hash;
  - `examples/project-template/AGENTS.md` `:66`;
  - `src/modules/profile/view.rs` `:8`, `:81`, `:94` (drop the "[planned]" comments);
  - `handlers.rs` `:219` and `tests/templates.rs` `:23` already match.
- takumi facts (read-only):
  - `takumi-core-0.31.0/src/resources/image.rs:491`: `ImageSource::from_bytes` (animated WebP/APNG → `Animated`, decoded lazily per frame);
  - `resources/animated.rs:165`;
  - `takumi-raster` draws `time_ms`.
- twilight: `User { id, avatar: Option<ImageHash>, discriminator: u16 }`, `Guild { id, icon }`, `ImageHash::{new, bytes, is_animated}`. Its `Display` gives the `a_` prefix. Store the hash as `([u8; 16], bool)`, never as `ImageHash`, because its `Deserialize` is not postcard-safe.

## Tasks & Acceptance

**Execution:**
- `Cargo.toml` -- add `hyper-rustls` to `[workspace.dependencies]` (comment: "AssetFetcher only (AD-15); deps-check `asset-http`") -- AC 1.
- `crates/noctis-image/Cargo.toml` -- the optional deps and features from Boundaries, plus `twilight-model` and `futures-util` (workspace); tokio gains `rt` and `time`; dev tokio gains `net`, `io-util`, `time` and `rt-multi-thread` -- enables the fetcher.
- `crates/noctis-ports/src/render.rs` (+ tests) -- `RenderCap::RemoteImages`, the two errors (message tests), `RendererPorts`, `Renderer::bind` -- port seam.
- `crates/noctis-core/src/app.rs`, `render.rs` -- the `bind` call and the doc. Test `img_03_build_binds_the_namespaced_asset_store`, which uses a recording fake Renderer -- AC 2.
- `crates/noctis-image/src/remote.rs` (+ `remote/tests.rs`):
  - `RemoteImage` (private `Source` enum; `avatar(&User, u16)`, `guild_icon(&Guild, u16)`, `url(&str) -> Result<Self, RemoteUrlError>`, `placeholder()`, `Display`; redacted `Debug`; `Serialize`/`Deserialize`/`Clone`/`PartialEq`/`Eq`/`Hash`);
  - `RemoteUrlError`; `normalize_size`; the URL builder; the cache key.
  - Tests `img_03_*` for every URL, size, avatar and icon row.
- `crates/noctis-image/src/remote/config.rs` (+ tests) -- `RemoteConfig` (the setters `allow_origin`, `discord_cdn`, `timeout`, `max_bytes`, `concurrency`, `max_per_job`, `max_pixels`, `cache_ttl`, `fallback`; `Default`; `PartialEq`/`Eq`/`Debug`/`Clone`), `Origin` parse and match, and `validate() -> Vec<String>` -- the origin, range and default rows.
- `crates/noctis-image/src/remote/declare.rs` -- hidden `TemplateAssets` (`const DECLARES: bool = false; fn declare(&self, _: &mut DeclaredImages) {}`), sealed `AssetField` with the IMG-03 `on_unimplemented`, implemented for the 3 types, and `DeclaredImages`/`Declared { field: Cow<'static, str>, required, image }`.
- `crates/noctis-image/src/remote/fetch.rs` (+ `fetch/tests.rs`, with a raw tokio `TcpListener` HTTP/1.1 test server helper that records requests, in-flight max and delays) -- `AssetFetcher::{new(&RemoteConfig) -> Result<Self, String>, resolve(..)}`: semaphore, timeout, `Limited` body, no redirects, sniff, pixel cap, `spawn_blocking` decode, cache get and put, warnings -- the fetch, cache, concurrency and cancel rows. This is the only file in `noctis-image/src` that names `legacy::Client`.
- `crates/noctis-image/src/{template,node/mod,lower,renderer,plugin,lib}.rs` (+ their tests) -- the wiring in the Code Map; render-level tests for the happy, required, dedupe, count-cap, undeclared, unbound, no-asset and animated rows.
- `crates/noctis-macros/src/template.rs`, `view.rs` (+ tests) -- the derive and `view!` rows.
- `crates/noctis/src/lib.rs` -- re-exports and docs.
- `crates/noctis/tests/remote_images.rs` (new, `#![cfg(feature = "image")]`) -- end to end through `App::build` on `Runtime::for_test()`:
  - `ImageConfig::new().font(..).remote(RemoteConfig::new().discord_cdn("http://127.0.0.1:<port>"))`;
  - a custom recording `AssetStore` set with `.asset_store(..)`;
  - `RemoteImage::avatar(&user, 256)` drawn and cached under the namespaced key;
  - a non-allow-listed URL gives the fallback pixel and 0 requests;
  - a required failure is `RenderError::RemoteImage`.
- `crates/noctis/tests/ui-image/img/IMG-03_asset_field_type.{rs,stderr}` -- `#[asset] a: String`. Generate it with `TRYBUILD=overwrite`, then review it.
- `xtask/src/deps_check.rs` (+ tests `asset_http_*`) -- rule `asset-http`:
  1. No member depends, in any kind, on `reqwest`, `ureq`, `isahc`, `surf`, `attohttpc`, `curl`, `awc`, `minreq` or `hyper-tls`.
  2. Only `noctis-image` depends on `hyper-rustls`, as an optional normal dependency enabled only by `render`.
  3. Among `noctis-image/src/**/*.rs` (minus test files), only `src/remote/fetch.rs` contains `legacy::Client`.

  Messages end `(AD-15) [asset-http]`.
- `crates/noctis-rules/src/rule.rs`, `docs/discord-rules.md` §16 -- the **IMG-03** Remote images row:
  - **Limit/value:** declared `#[asset]` fields; the default allow-list; the origin grammar; the ranges and defaults; CDN sizes as powers of two 16..=4096 **[D]**.
  - **Source:** [REF §Image Formatting](https://discord.com/developers/docs/reference#image-formatting).
  - **Enforcement:** `Compile-time / App-build / Runtime`.
  - **How:** the `on_unimplemented` and derive errors; the plugin violations; `RenderError::RemoteImage`/`UndeclaredImage`/`OverCap { RemoteImages }`; the fallback and warning.
  - Evidence: the fixture above, plus `img_03_*` tests.
- Skill and template:
  - **Usage Skill.** `images.md` §7 gets the real API: declaring, the constructors, `RemoteConfig` and the allow-list, failures, the fallback, required, caching keys and TTL, animated avatars, the error variants. Remove every "[planned, Story 10.8]" and the troubleshooting row; add IMG-03 to the limits cheatsheet.
  - **Template.** Its `AGENTS.md` and `profile/view.rs` comments.
  - **API.** Re-bless `xtask/public-api.txt` and copy the printed hash into `SKILL.md`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already holds the planning decisions (added by the planner).
- `.github/workflows-disabled/ci.yml` -- confirm only; `deps-check` already runs, so there is no new gate.

**Acceptance Criteria:**
- Given the deferred HTTP client choice, when the story is done, then the Design Notes and the memlog record `hyper-util` legacy `Client` + `hyper-rustls` 0.27.10, `[workspace.dependencies]` pins `hyper-rustls = "0.27.10"`, `Cargo.lock` has no new package, and `cargo run --release -p xtask -- deps-check` passes with the `asset-http` rule, whose unit tests prove each of its three violations.
- Given a Template with `#[asset]` fields, when `ctx.render`/`app.renderer().render(job)` runs, then every declared image is fetched (or read from the `AssetStore`) and decoded before the job is submitted to the render pool. The decode runs on tokio's blocking pool under the per-App semaphore, and a second identical render makes no HTTP request.
- Given a URL whose origin is not allow-listed, when a render declares it, then the server receives no request, the fallback is drawn and one `noctis::image` warning names the origin.
- Given a fetch that times out, exceeds `max_bytes` or `max_pixels`, is not PNG/JPEG/WebP, or fails to decode, when the render runs, then the configurable fallback is drawn with a warning. The same field marked `#[asset(required)]` makes the render return `RenderError::RemoteImage`.
- Given `RemoteImage::avatar` and `guild_icon`, when they build a cache key, then it holds the id, the asset hash, the normalized size and the animated flag. A changed hash is a new key, and an `a_` avatar drawn by an animated Template animates: output frames differ, following the source frames.
- Given the new public API, when the story is done, then the Usage Skill documents remote images and the allow-list, and `skill-check`, `api-snapshot`, `rules-catalog` and `cargo deny check` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why hyper-util + hyper-rustls, not reqwest.**
  - Both are already in the lock (through twilight-http and core), share the ring, rustls and platform-verifier stack, and add zero crates.
  - The legacy `Client` does not follow redirects, which is exactly the allow-list's safety property.
  - reqwest would add a second TLS and cookie surface, plus redirect policy to configure, for one GET.
- **Origins, not bare hosts.** Template data cannot switch the scheme or port, and loopback-only plain http lets tests and local media proxies work through a production override point (`discord_cdn`, `allow_origin`) instead of a test-only branch (AD-24).
- **Resolution in the Renderer, binding through the port.** Core cannot name `noctis-image` types, and plugins install before ports resolve. So `Renderer::bind` hands over the namespaced `AssetStore` after `Ports::resolve`. It is a provided method, so existing fake Renderers compile unchanged. Story 10.10 can add `render_cache` to the `#[non_exhaustive]` `RendererPorts`.
- **Admission after resolution.** Holding a pool slot or thread while waiting on the network would starve renders. A Template with remote images is therefore admitted after its fetches, so its `QueueFull` can arrive after fetch time; this is documented in the skill. Templates without `#[asset]` keep call-order admission.
- **Hidden traits from the derive.** `TemplateAssets` keeps hand-written test Templates to a one-line impl. `AssetField` makes a wrong field type a compile error tagged `[IMG-03]` (maintainer decision 4: compile time first). An undeclared remote `<img>` is a typed runtime error, never a silent fetch.
- **Pixel cap default of 4 MP.** A decoded still is RGBA in memory, so 4 MP is 16 MiB, times the concurrency of 8. 2048-pixel avatars fit; larger ones need `max_pixels`.

## Verification

**Commands** (run in `/home/claude/lanes/a/noctis-rs`):
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` and `cargo test --release --workspace --all-features` -- pass
- `cargo hack test --release --workspace --each-feature` -- pass
- `TRYBUILD=overwrite cargo test --release -p noctis --test ui --features image` once, review the new `.stderr`, then rerun without `TRYBUILD` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `git diff --stat Cargo.lock` adds no `[[package]]`; `cargo tree -p noctis-image --features render -i takumi-core` shows a single 0.31.0; `cargo tree -p noctis-image --features render -i hyper-rustls` shows 0.27.10
- `bash tools/story-loop/gates.sh` -- ALL-OK
