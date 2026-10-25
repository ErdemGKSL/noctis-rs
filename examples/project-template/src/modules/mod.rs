//! One Noctis Module per feature (the tescord "Pack"), one folder each:
//! `mod.rs` (the Module and its Routes), `view.rs` (render functions, the single source
//! of truth for each message), `handlers.rs` (Commands, component and modal handlers)
//! and, when the feature listens to the gateway, `events.rs`. Each exposes `module()`.
//! `ops` holds only Lifecycle Hooks (`#[derive(Hook)]`); `voice` exists only with the
//! template's `voice` feature.

pub mod dashboard;
pub mod ops;
pub mod profile;
pub mod setup;
#[cfg(feature = "voice")]
pub mod voice;
