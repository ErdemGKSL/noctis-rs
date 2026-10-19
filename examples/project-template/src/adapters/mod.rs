// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Our own Adapter implementations (FR-38). Each one implements a port trait from
//! `noctis::ports` and is handed to the App builder; handler code never changes.

mod bounded_cache;

pub use bounded_cache::BoundedCacheStore;
