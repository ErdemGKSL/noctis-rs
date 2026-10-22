// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Typed strings compiled from `locales/*.yaml` (AD-16, FR-42).
//!
//! `i18n!` generates, inside this module:
//! - `pub struct Strings`: `Strings::of(locale)` picks `locale`'s file, else its sibling's
//!   (`en-GB` ↔ `en-US`, `es-ES` ↔ `es-419`), else `en-US.yaml`. Its fields are the root
//!   maps (nested node structs mirroring the YAML tree) and its methods the root leaves. A
//!   leaf without placeholders returns `&'static str`; one with placeholders takes one
//!   `impl Display` per distinct name, in order of first appearance, and returns a `String`
//!   with every occurrence replaced: `s.setup.progress(n, total, title)`.
//! - `pub mod keys`: one `MessageKey` const per content key, dots to `_`, uppercased
//!   (`keys::PROFILE_BADGE_UNKNOWN`), for `HandlerError::user`, `defaults(label = keys::X)`
//!   and per-render overrides.
//! - `pub fn catalog()`: this crate's catalog; `AppBuilder::locales(strings::catalog())`
//!   (in `lib.rs`) installs it, so every key and `noctis.*` override resolves from these
//!   files. `Module::locales` (per-Module catalogs) is [planned, Story 11.2].
//!
//! File names are Discord locale codes (`en-US.yaml`, `tr.yaml`). A key missing from
//! `en-US.yaml` (the default locale) or a call with the wrong arguments is a compile
//! error. A key missing from `tr.yaml` is one `[LOC-04]` `deprecated` build warning and
//! falls back to English. Keys under `cmd.`, `cmdword.` and `ctxcmd.` feed `#[command]`
//! and Publish only, and `noctis.` overrides the built-in notices; none of them get
//! accessors.

noctis::i18n!(default = "en-US", dir = "locales");
