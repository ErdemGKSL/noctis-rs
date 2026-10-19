// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Typed strings compiled from `locales/*.yaml` (AD-16, FR-42).
//!
//! `i18n!` generates, inside this module:
//! - `Strings::of(locale)`: an accessor whose nested fields mirror the YAML tree, with one
//!   method per leaf (`s.setup.step_of(step, total, title)`), arguments in placeholder order;
//! - `keys::*`: one `MessageKey` const per key (`keys::PROFILE_NOT_FOUND`);
//! - `catalog()`: the descriptor `AppBuilder::locales` merges at App build.
//!
//! File names are Discord locale codes (`en-US.yaml`, `tr.yaml`). A key missing from
//! `en-US.yaml` (the default locale) or a call with the wrong arguments is a compile
//! error. A key missing from `tr.yaml` falls back to English with a build warning.
//! Keys under `cmd.`, `cmdword.` and `ctxcmd.` feed `#[command]` and Publish only; they
//! get no accessors.

noctis::i18n!(default = "en-US", dir = "locales");
