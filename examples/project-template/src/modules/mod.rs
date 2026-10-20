// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! One Noctis Module per feature (the tescord "Pack"), one folder each:
//! `mod.rs` (the Module and its Routes), `view.rs` (render functions, the single source
//! of truth for each message), `handlers.rs` (Commands, component and modal handlers)
//! and, when the feature listens to the gateway, `events.rs`. Each exposes `module()`.

pub mod dashboard;
pub mod profile;
pub mod setup;
