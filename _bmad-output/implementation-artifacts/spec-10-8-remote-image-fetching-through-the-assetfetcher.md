---
title: 'Story 10.8: Remote image fetching through the AssetFetcher'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '26c6132c5481cc1c56875d31609fe1901d10db8c'
review_loop_iteration: 0
followup_review_recommended: true
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
deferred:
  - summary: >-
      The story-loop harness change to AGENTS.md allows debug builds, contradicting maintainer decision 6 (release builds only) without updating maintainer-decisions.md.
    evidence: |-
      Commit 8fed800 rewrites the AGENTS.md "Running and verifying" bullet to "Debug builds are allowed (maintainer, 2026-10-24)"; maintainer-decisions.md section 6 still says every build, test, check and run uses --release, and that page requires the contradicting document to be fixed in the same change.
    location: >-
      AGENTS.md (Running and verifying); _bmad-output/planning-artifacts/maintainer-decisions.md section 6
    severity: medium
  - summary: >-
      Unrelated story-loop tooling, AGENTS.md and noctis-testkit changes were swept into the Story 10.8 implementation commit.
    evidence: |-
      git diff 26c6132..HEAD touches tools/story-loop/{README.md,bg.sh,cargo-queue.sh,env.sh,epic-last.py,gates.sh,lane.sh,tree-sync.py}, AGENTS.md and crates/noctis-testkit/tests/session_resume.rs, none of which Story 10.8's intent covers.
    location: >-
      tools/story-loop/, AGENTS.md, crates/noctis-testkit/tests/session_resume.rs
    severity: low
  - summary: >-
      gates.sh no longer fails on a target/debug directory and dates the cargo-hack rule 2026-10-24 while AGENTS.md cites 2026-10-23.
    evidence: |-
      The line `test ! -d target/debug || { echo "FAIL:debug-dir"; exit 1; }` was removed and the cargo-queue comment dropped; harness change, not story code.
    location: >-
      tools/story-loop/gates.sh
    severity: low
  - summary: >-
      epic-last.py ignores a failed git fetch or git show of origin/main, so stories finished in other lanes count as open and cargo hack can be skipped for an epic's real last story.
    evidence: |-
      Both subprocess.run calls discard their return codes; an empty stdout yields an empty `main` map.
    location: >-
      tools/story-loop/epic-last.py
    severity: low
  - summary: >-
      bg.sh can exit with an empty status when two attached callers race on consuming the job directory.
    evidence: |-
      The first caller moves $J away about 1 s after seeing status; a second caller that checked later then runs `cat "$J/status"` on a moved directory, gets an empty `st`, and `exit ""` fails with a non-numeric-argument error.
    location: >-
      tools/story-loop/bg.sh
    severity: low
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

### 2026-10-24 — Review pass

- verdicts: 38 findings — high 0, medium 5, low 22, false 11, maybe-false 0
- findings:
  - `[low]` `[patch]` (blind) `RemoteImage::url` accepts a 2048-byte URL with an empty path whose canonical form gains a `/` (2049 bytes), so `target()` re-parses it as `TooLong` and it always draws the fallback — verified in `ParsedUrl::canonical` and `RemoteImage::target`; patch queued: enforce the limit on the canonical form in `url()` and test the boundary.
  - `[low]` `[reject]` (blind) `AssetStore` `get`/`put` run outside the deadline while the permit is held — real only with a stalling custom adapter (the only shipped adapter is `MemoryAssetStore`); the spec bounds permit wait, request and body only, and adding store timeouts is new guarding for a state not demonstrated.
  - `[false]` `[reject]` (blind) decode (and cache-hit decode) runs outside the deadline — the spec states the timeout does not cover the decode and `RemoteConfig::timeout`'s doc says the same; takumi decodes one frame at `from_bytes`, and the pixel cap bounds it.
  - `[low]` `[reject]` (blind) the warning says "drawing the fallback" also for required images — the message text is the one the spec prescribes for every failed image, and the render error reported to the sink disambiguates; changing it adds a branch for a cosmetic log wording.
  - `[low]` `[reject]` (blind) `join_all` waits for the other fetches after a required image failed — every fetch is bounded by `timeout`; a short-circuit adds abort logic for a latency-only gain.
  - `[low]` `[patch]` (blind) `RemoteImage::url` docs say `http://` only on a loopback host but the constructor accepts any http host — verified in `ParsedUrl::parse` (only `Scheme::split`); a non-loopback http origin can never be allow-listed, so it draws the fallback; patch queued: correct the rustdoc and the skill's `images.md` line.
  - `[low]` `[reject]` (blind) `RemoteUrlError::Malformed` names the bad-port case — the spec's URL-parts matrix row prescribes `Malformed` for `https://h:99999/x`; the fix edits this build's spec.
  - `[low]` `[reject]` (blind) the host grammar accepts `.`, `-`, `a..b`, trailing dots and decimal hosts — the spec's origin grammar (`[a-z0-9.-]+`) settles this; matching is exact, so a nonsense URL host is simply not allow-listed, and a nonsense `allow_origin` is the operator's own config; extra label rules are new guards.
  - `[low]` `[patch]` (blind) `Origin::parse` matches the scheme case-sensitively while `Scheme::split` for URLs ignores case — verified (`match scheme { "https" | "http" }` vs `eq_ignore_ascii_case`); patch queued: compare the origin scheme case-insensitively and test `HTTPS://i.imgur.com`.
  - `[low]` `[reject]` (blind) loopback is compared as exact strings and IPv6 literals are not canonicalized — the spec lists exactly `127.0.0.1`, `[::1]` and `localhost`; a differently written loopback is refused at build or not matched (safe failure), rare in practice, and normalization adds parsing branches.
  - `[low]` `[reject]` (blind) an explicit default port (`:443`) keeps its own cache key and dedup entry — only a duplicate fetch/cache entry when one job mixes both spellings, which constructors never do; dropping default ports adds canonicalization branches.
  - `[low]` `[reject]` (blind) `plan` dedups on the `RemoteImage` value, not on the resolved target — differing unnormalized sizes only arise from hand-crafted job data (constructors normalize); worst case a duplicate fetch counted against `max_per_job`.
  - `[low]` `[reject]` (blind) an `#[asset]` Template decodes its postcard data once more — a small struct decode per render; no named harm.
  - `[false]` `[reject]` (blind) a fallback render could be served from the core `RenderCache` — `noctis-core/src/render.rs` does not consult `RenderCache` yet; the render cache and its key (which includes asset pairs) are Story 10.10, which the intent excludes.
  - `[false]` `[reject]` (blind) hand-written Templates need `impl TemplateAssets` and the skill does not say so — the derive always emits the impl; the Usage Skill never shows a hand-written `impl Template`, only `#[derive(Template)]` plus `impl TemplateView`.
  - `[low]` `[defer]` (blind) `gates.sh` drops the `target/debug` guard and dates the hack rule 2026-10-24 — story-loop harness change swept into the story commit (consistent with the harness's new AGENTS.md text allowing debug builds); not this story's implementation.
  - `[low]` `[defer]` (blind) `epic-last.py` ignores a failed `git fetch`/`git show`, so `cargo hack` can be skipped for the real last story — harness tooling, not this story's code.
  - `[low]` `[reject]` (edge) store `get`/`put` not bounded by a timeout — same root cause as the blind finding above; same reason.
  - `[medium]` `[patch]` (edge) a dropped render future releases the semaphore permit while the `spawn_blocking` decode keeps running — verified: `_permit` lives in `fetch`'s future and `decode` awaits a detached blocking task, so cancellations let decodes exceed `concurrency`, against the spec's "one permit is held for … decode"; patch queued: `Arc<Semaphore>` with `acquire_owned`, permit moved into the blocking closure.
  - `[false]` `[reject]` (edge) animated WebP with many frames exhausts memory — takumi-core 0.31.0 `AnimatedSource::from_bytes` decodes only one frame (`Some(1)`) and never holds decoded frames; each frame is decoded per draw and bounded by the pixel cap.
  - `[false]` `[reject]` (edge) images are fetched before a `QueueFull` rejection — the spec's Design Notes deliberately admit asset Templates after their fetches and document the late `QueueFull`.
  - `[low]` `[reject]` (edge) one Renderer handed to two Apps keeps the first App's store (`bind` first-wins) — the spec prescribes first-call-wins; the plugin builds one Renderer per App, sharing one is unusual, and a warning is a new guard.
  - `[low]` `[reject]` (edge) required failure logs "drawing the fallback" — same root cause as the blind log-wording finding; same reason.
  - `[low]` `[patch]` (edge) `RemoteImage::url` doc vs. any-host http — same root cause as the blind doc finding; same patch.
  - `[medium]` `[patch]` (vgap) the fallback warning and its query-free `image` field are never asserted — pre-verified; AC 3 and the no-query display rule are unguarded; patch queued: a capturing-subscriber test in `remote/fetch/tests.rs`.
  - `[medium]` `[patch]` (vgap) the timeout is verified only for a stalled response head, not for a stalled body or the permit wait — pre-verified; patch queued: `img_03_timeout_covers_body_and_permit`.
  - `[low]` `[patch]` (vgap) a configured `RemoteConfig::fallback` is never drawn in a test — pre-verified; patch queued: a renderer test with a custom fallback for a 404 and for the placeholder.
  - `[low]` `[defer]` (vgap other) `bg.sh` can `exit ""` when two attached callers race on consuming the job directory — harness tooling swept into the story commit, not this story's code.
  - `[false]` `[reject]` (intent D1) origin allow-list on `RemoteConfig` instead of a host list on `ImageConfig` — the intent contract deliberately settles origins, their grammar and the default list; the skill was rewritten to match.
  - `[false]` `[reject]` (intent D2) `RemoteImage::emoji` dropped — the intent's Never list excludes a custom-emoji constructor (Story 10.9).
  - `[false]` `[reject]` (intent D3) the fetch runs as async I/O on the runtime — non-blocking I/O does not block the runtime; the intent puts the decode on the blocking pool, which the code does.
  - `[false]` `[reject]` (intent D4) a not-allow-listed image in a required field fails the render — the intent's matrix row "Not allow-listed" says "fallback, or `RemoteImage` when required".
  - `[medium]` `[patch]` (intent D5) "with a log" is not verified — same root cause as the vgap warning finding; same patch.
  - `[low]` `[reject]` (intent D6) e2e tests call `app.renderer().render` rather than `ctx.render` — `ctx.render` delegates to the same Renderer; the spec prescribes this e2e shape; a handler-level test adds harness for no new path.
  - `[false]` `[reject]` (intent D7) `hyper-rustls` is a caret requirement, not `=0.27.10` — the spec prescribes `version = "0.27.10"`, and `Cargo.lock` fixes the version as everywhere in the workspace.
  - `[false]` `[reject]` (intent D8) new port API (`Renderer::bind`, `RendererPorts`, new errors) — the intent names `Renderer::bind(RendererPorts)` and the spec designs the rest; API snapshot and skill are updated.
  - `[low]` `[defer]` (intent D9) unrelated story-loop tooling, `AGENTS.md` and testkit changes are bundled into the story diff — harness changes made outside the story's implementation and swept into its commit.
  - `[medium]` `[defer]` (intent D10) the bundled `AGENTS.md` change allows debug builds, contradicting maintainer decision §6 ("Release builds only") without updating `maintainer-decisions.md` — real; its fix edits agent-context files (AGENTS.md / the decisions page), so it is deferred to the maintainer's harness owner.

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

## Auto Run Result

- **Summary:** `#[asset]` Template fields of type `RemoteImage` are fetched by the `noctis-image` AssetFetcher (hyper-util + hyper-rustls, no redirects) against an origin allow-list on `RemoteConfig` (Discord CDN by default, loopback-only plain http), under a semaphore (`concurrency`), a per-image `timeout` covering permit wait, request and body, a byte cap, a 4 MP pixel cap, and `max_per_job`. Decoding runs on the blocking pool; results are cached in the namespaced `AssetStore` bound through `Renderer::bind(RendererPorts)`. Optional images that fail draw `RemoteConfig::fallback` with a `noctis::image` warning (query-free URL); required ones fail the render with a typed error.
- **Files changed (review pass):** `crates/noctis-image/src/remote.rs` (URL length enforced on the canonical form, doc fix for http hosts); `remote/config.rs` (case-insensitive origin scheme); `remote/fetch.rs` (`Arc<Semaphore>` + `acquire_owned`, permit moved into the blocking decode); `remote/fetch/tests.rs` (warning capture, body/permit timeout tests); `remote/config/tests.rs`, `remote/tests.rs`, `renderer/tests/remote.rs` (boundary, scheme-case and custom-fallback tests); `crates/noctis-image/Cargo.toml` + `Cargo.lock` (dev-dep `tracing-subscriber`); `examples/project-template/.claude/skills/noctis/references/images.md` (http wording). Implementation files are in commit 8fed800.
- **Review findings:** 38 total (high 0, medium 5, low 22, false 11). Patched: 4 medium (permit released on cancellation; warning not asserted; timeout not verified for body/permit; "with a log" unverified — same patch as warning) and 5 low (canonical-URL length, http doc x2, origin scheme case, custom fallback test). Deferred: 5 (story-loop harness: gates.sh debug guard, epic-last.py fetch errors, bg.sh exit race, bundled harness changes, AGENTS.md debug-build text vs maintainer decision §6). Rejected: 24, each with its reason in the Review Triage Log (spec-prescribed behavior, out of scope for 10.9/10.10, or new guards without demonstrated harm).
- **Follow-up review recommended: true** (first pass, 4 medium patched, 0 high). Named risk: the semaphore change (owned permit moved into the `spawn_blocking` decode) alters cancellation/concurrency semantics and is verified only by the new tests, not by an independent review.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy, workspace tests, api-snapshot, deps-check, rules-catalog, skill-check, tw-grammar, tw_crosscheck, cargo deny, MSRV 1.91). CI paused by the maintainer.
- **Residual risks:** `AssetStore` get/put are not deadline-bounded (only matters for a stalling custom adapter); loopback/IPv6 and default-port spellings are not canonicalized; asset Templates may get `QueueFull` after fetch time (documented); harness AGENTS.md debug-build text conflicts with maintainer decision §6 (deferred).
