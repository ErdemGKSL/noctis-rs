//! `State<T>`: typed shared state registered at build (AD-9).

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Shared, read-only state of type `T`, registered with
/// [`AppBuilder::state`](crate::AppBuilder::state) (one value per type).
///
/// Mutable state keeps its interior mutability inside `T` (atomics,
/// `tokio::sync::RwLock`) or, better, behind a port.
///
/// ```no_run
/// use noctis_core::App;
///
/// struct BotMeta {
///     name: &'static str,
/// }
///
/// # #[tokio::main] async fn main() -> Result<(), noctis_core::BuildError> {
/// let app = App::builder().state(BotMeta { name: "noctis" }).build()?;
/// let meta = app.state::<BotMeta>().expect("registered above");
/// assert_eq!(meta.name, "noctis");
/// # Ok(()) }
/// ```
pub struct State<T>(Arc<T>);

impl<T> State<T> {
    pub(crate) fn from_arc(value: Arc<T>) -> Self {
        Self(value)
    }
}

impl<T> Deref for State<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: fmt::Debug> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("State").field(&*self.0).finish()
    }
}

/// One value per type; `Debug` prints type names only, never values.
#[derive(Default)]
pub(crate) struct StateMap(HashMap<TypeId, (&'static str, Arc<dyn Any + Send + Sync>)>);

impl StateMap {
    /// Inserts `value` unless a value of `T` is present; `false` keeps the
    /// first one.
    pub(crate) fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> bool {
        let id = TypeId::of::<T>();
        if self.0.contains_key(&id) {
            return false;
        }
        self.0.insert(id, (type_name::<T>(), Arc::new(value)));
        true
    }

    pub(crate) fn get<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        let (_, value) = self.0.get(&TypeId::of::<T>())?;
        Arc::clone(value).downcast::<T>().ok().map(State::from_arc)
    }
}

impl fmt::Debug for StateMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names: Vec<&str> = self.0.values().map(|(name, _)| *name).collect();
        names.sort_unstable();
        f.debug_list().entries(names).finish()
    }
}
