//! The `Plugin` seam optional crates attach through (AD-24).

use std::any::{Any, TypeId, type_name};

use crate::AppBuilder;

/// An optional feature that configures an App through the public
/// [`AppBuilder`] methods (`renderer`, port setters, `intents`, `runtime`,
/// [`report`](AppBuilder::report)).
///
/// [`AppBuilder::build`] calls every plugin's `install` exactly once, in
/// registration order, before its checks. A plugin added during an `install`
/// is installed after the current batch, in the same build. One plugin per
/// type: a second one is a
/// [`BuildViolation::DuplicatePlugin`](crate::BuildViolation::DuplicatePlugin).
/// A panicking `install` panics `build`.
///
/// ```
/// use noctis_core::{App, AppBuilder, Plugin};
/// use noctis_ports::memory::MemoryCacheStore;
///
/// struct SmallCache;
///
/// impl Plugin for SmallCache {
///     fn install(&self, app: &mut AppBuilder) {
///         app.cache_store(MemoryCacheStore::new());
///     }
/// }
///
/// let mut builder = App::builder();
/// builder.plugin(SmallCache);
/// ```
pub trait Plugin: Send + Sync + 'static {
    /// Configures `app`; called once by [`AppBuilder::build`].
    fn install(&self, app: &mut AppBuilder);
}

/// A [`Plugin`] behind `dyn`, reachable as `&mut P` again by
/// [`AppBuilder::plugin_mut`].
pub(crate) trait ErasedPlugin: Send + Sync + 'static {
    fn install(&self, app: &mut AppBuilder);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<P: Plugin> ErasedPlugin for P {
    fn install(&self, app: &mut AppBuilder) {
        Plugin::install(self, app);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A registered plugin waiting for its `install`.
pub(crate) struct PluginEntry {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) plugin: Box<dyn ErasedPlugin>,
}

impl PluginEntry {
    pub(crate) fn new<P: Plugin>(plugin: P) -> Self {
        Self {
            type_id: TypeId::of::<P>(),
            type_name: type_name::<P>(),
            plugin: Box::new(plugin),
        }
    }
}
