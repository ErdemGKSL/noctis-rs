//! The process `Runtime` and its frozen, type-erased resources (AD-24).

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

/// A process-scoped resource held by a [`Runtime`], such as a render pool.
///
/// The first [`Runtime::resource_or_init`] call for a type runs
/// [`init`](Resource::init) and freezes its configuration; later calls with an
/// equal configuration get the same value, a different one is a
/// [`ResourceConflict`].
///
/// A configuration must not hold secrets: its [`Debug`] output is printed in
/// [`ResourceConflict`].
///
/// ```
/// use noctis_core::{Resource, Runtime};
///
/// struct Pool {
///     threads: usize,
/// }
///
/// impl Resource for Pool {
///     type Config = usize;
///
///     fn init(threads: &usize) -> Self {
///         Self { threads: *threads }
///     }
/// }
///
/// let runtime = Runtime::for_test();
/// let pool = runtime.resource_or_init::<Pool>(4)?;
/// assert_eq!(pool.threads, 4);
/// # Ok::<(), noctis_core::ResourceConflict>(())
/// ```
pub trait Resource: Send + Sync + 'static {
    /// The configuration the first caller freezes; compared with `==`.
    type Config: PartialEq + fmt::Debug + Send + Sync + 'static;

    /// Creates the resource from the first caller's configuration.
    fn init(config: &Self::Config) -> Self;
}

/// A resource was requested with a configuration other than the frozen one.
///
/// ```
/// use noctis_core::{Resource, Runtime};
///
/// struct Fonts;
/// impl Resource for Fonts {
///     type Config = u8;
///     fn init(_: &u8) -> Self {
///         Fonts
///     }
/// }
///
/// let runtime = Runtime::for_test();
/// runtime.resource_or_init::<Fonts>(1)?;
/// let conflict = runtime.resource_or_init::<Fonts>(2).err().unwrap();
/// assert_eq!((conflict.frozen.as_str(), conflict.requested.as_str()), ("1", "2"));
/// # Ok::<(), noctis_core::ResourceConflict>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error(
    "process resource `{resource}` is frozen with config {frozen}, but {requested} was \
     requested: give every App on one Runtime the same config, or attach the App to \
     another Runtime with `AppBuilder::runtime`"
)]
pub struct ResourceConflict {
    /// The resource's type name.
    pub resource: &'static str,
    /// `Debug` of the frozen configuration.
    pub frozen: String,
    /// `Debug` of the rejected configuration.
    pub requested: String,
}

/// The process-scoped home of resources shared by every App attached to it.
///
/// Apps attach to [`Runtime::global`] unless
/// [`AppBuilder::runtime`](crate::AppBuilder::runtime) names another; tests
/// use an isolated [`Runtime::for_test`]. Clones are handles to the same
/// Runtime, and equality is identity.
///
/// ```
/// use noctis_core::Runtime;
///
/// assert_eq!(Runtime::global(), Runtime::global());
/// assert!(!Runtime::global().is_test());
/// let test = Runtime::for_test();
/// assert!(test.is_test());
/// assert_ne!(&test, Runtime::global());
/// assert_eq!(test.clone(), test);
/// ```
#[derive(Clone)]
pub struct Runtime(Arc<Inner>);

type ResourceMap = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

struct Inner {
    test: bool,
    resources: Mutex<ResourceMap>,
}

/// A frozen resource with the configuration that created it.
struct Frozen<T: Resource> {
    config: T::Config,
    value: Arc<T>,
}

impl Runtime {
    fn new(test: bool) -> Self {
        Self(Arc::new(Inner {
            test,
            resources: Mutex::new(HashMap::new()),
        }))
    }

    /// The process Runtime every App attaches to by default.
    ///
    /// ```
    /// assert!(!noctis_core::Runtime::global().is_test());
    /// ```
    pub fn global() -> &'static Runtime {
        static GLOBAL: OnceLock<Runtime> = OnceLock::new();
        GLOBAL.get_or_init(|| Self::new(false))
    }

    /// A fresh, isolated Runtime for one test; never equal to
    /// [`global`](Self::global).
    ///
    /// ```
    /// use noctis_core::Runtime;
    ///
    /// assert_ne!(Runtime::for_test(), Runtime::for_test());
    /// ```
    #[must_use]
    pub fn for_test() -> Self {
        Self::new(true)
    }

    /// Whether this Runtime came from [`for_test`](Self::for_test).
    ///
    /// ```
    /// assert!(noctis_core::Runtime::for_test().is_test());
    /// ```
    pub fn is_test(&self) -> bool {
        self.0.test
    }

    /// The resource of type `T`, created from `config` on first use.
    ///
    /// `init` runs outside the Runtime's lock, so it may itself request other
    /// resources; when two callers race, one `init` result wins and the other
    /// is dropped.
    ///
    /// # Errors
    ///
    /// [`ResourceConflict`] when `T` is already frozen with a configuration
    /// that is not equal to `config`.
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use noctis_core::{Resource, Runtime};
    ///
    /// struct Emoji;
    /// impl Resource for Emoji {
    ///     type Config = ();
    ///     fn init(_: &()) -> Self {
    ///         Emoji
    ///     }
    /// }
    ///
    /// let runtime = Runtime::for_test();
    /// let a = runtime.resource_or_init::<Emoji>(())?;
    /// let b = runtime.resource_or_init::<Emoji>(())?;
    /// assert!(Arc::ptr_eq(&a, &b));
    /// # Ok::<(), noctis_core::ResourceConflict>(())
    /// ```
    pub fn resource_or_init<T: Resource>(
        &self,
        config: T::Config,
    ) -> Result<Arc<T>, ResourceConflict> {
        let existing = self.lock().get(&TypeId::of::<T>()).cloned();
        if let Some(existing) = existing {
            return compare(&downcast::<T>(existing), &config);
        }
        let value = Arc::new(T::init(&config));
        let mut map = self.lock();
        match map.entry(TypeId::of::<T>()) {
            MapEntry::Occupied(entry) => {
                let existing = downcast::<T>(Arc::clone(entry.get()));
                drop(map);
                compare(&existing, &config)
            }
            MapEntry::Vacant(entry) => {
                entry.insert(Arc::new(Frozen::<T> {
                    config,
                    value: Arc::clone(&value),
                }));
                Ok(value)
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, ResourceMap> {
        self.0
            .resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// The frozen entry of `T`, from the map slot keyed by `TypeId::of::<T>()`.
#[expect(
    clippy::expect_used,
    reason = "invariant: the slot for TypeId::of::<T>() only ever holds a Frozen<T>"
)]
fn downcast<T: Resource>(entry: Arc<dyn Any + Send + Sync>) -> Arc<Frozen<T>> {
    entry
        .downcast::<Frozen<T>>()
        .expect("resource slot holds the Frozen<T> of its TypeId")
}

fn compare<T: Resource>(
    frozen: &Frozen<T>,
    requested: &T::Config,
) -> Result<Arc<T>, ResourceConflict> {
    if frozen.config == *requested {
        Ok(Arc::clone(&frozen.value))
    } else {
        Err(ResourceConflict {
            resource: type_name::<T>(),
            frozen: format!("{:?}", frozen.config),
            requested: format!("{requested:?}"),
        })
    }
}

impl PartialEq for Runtime {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Runtime {}

impl fmt::Debug for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Runtime")
            .field("is_test", &self.0.test)
            .field("resources", &self.lock().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static INITS: AtomicUsize = AtomicUsize::new(0);

    struct Dummy(u32);

    impl Resource for Dummy {
        type Config = u32;

        fn init(config: &u32) -> Self {
            INITS.fetch_add(1, Ordering::SeqCst);
            Self(*config)
        }
    }

    /// A resource used only by the freeze test, so its init count is exact.
    struct Counted;

    static COUNTED_INITS: AtomicUsize = AtomicUsize::new(0);

    impl Resource for Counted {
        type Config = u32;

        fn init(_: &u32) -> Self {
            COUNTED_INITS.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

    #[test]
    fn identity() {
        let (a, b) = (Runtime::for_test(), Runtime::for_test());
        assert_ne!(a, b);
        assert_eq!(Runtime::global(), Runtime::global());
        assert!(!Runtime::global().is_test());
        assert!(a.is_test());
        assert_ne!(&a, Runtime::global());
    }

    #[test]
    fn freeze_and_conflict() {
        let rt = Runtime::for_test();
        let a = rt.resource_or_init::<Counted>(1).unwrap();
        let b = rt.resource_or_init::<Counted>(1).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(COUNTED_INITS.load(Ordering::SeqCst), 1);
        let err = rt.resource_or_init::<Counted>(2).err().unwrap();
        assert_eq!(err.frozen, "1");
        assert_eq!(err.requested, "2");
        assert!(err.resource.ends_with("Counted"));
        assert!(format!("{rt:?}").contains("resources: 1"));
    }

    #[test]
    fn isolation() {
        let (a, b) = (Runtime::for_test(), Runtime::for_test());
        assert_eq!(a.resource_or_init::<Dummy>(1).unwrap().0, 1);
        assert_eq!(b.resource_or_init::<Dummy>(2).unwrap().0, 2);
    }

    #[test]
    fn race() {
        let rt = Runtime::for_test();
        let arcs: Vec<Arc<Dummy>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| s.spawn(|| rt.resource_or_init::<Dummy>(1).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(arcs.iter().all(|a| Arc::ptr_eq(a, &arcs[0])));
    }
}
