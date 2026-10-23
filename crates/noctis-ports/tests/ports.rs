//! The port traits are dyn-compatible, swappable and callable (AC1).

use std::collections::HashMap;
use std::error::Error as _;
use std::future::{Future, ready};
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use bytes::Bytes;
use noctis_ports::{
    AssetStore, BoxFuture, CacheStore, DiscordCache, GuildAccessList, IdentifyQueue, Lease,
    LocaleSource, ModuleStateStore, PortError, RateLimitStore, RefStore, RenderCache, RenderError,
    RenderJob, RenderOutput, Renderer, SessionStore, TemplateDescriptor,
};
use twilight_model::channel::{Channel, Message};
use twilight_model::gateway::event::Event;
use twilight_model::guild::{Guild, Member, Role};
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use twilight_model::user::User;

/// Polls a future that is ready on its first poll.
fn now<F: Future>(future: F) -> F::Output {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("future was not ready"),
    }
}

fn assert_port<T: ?Sized + Send + Sync + 'static>() {}

#[test]
fn every_port_is_dyn_compatible() {
    assert_port::<dyn RefStore>();
    assert_port::<dyn SessionStore>();
    assert_port::<dyn IdentifyQueue>();
    assert_port::<dyn ModuleStateStore>();
    assert_port::<dyn RateLimitStore>();
    assert_port::<dyn CacheStore>();
    assert_port::<dyn DiscordCache>();
    assert_port::<dyn RenderCache>();
    assert_port::<dyn AssetStore>();
    assert_port::<dyn LocaleSource>();
    assert_port::<dyn GuildAccessList>();
    assert_port::<dyn Renderer>();
    let _: Option<Arc<dyn RefStore>> = None;
    let _: Option<Arc<dyn SessionStore>> = None;
    let _: Option<Arc<dyn IdentifyQueue>> = None;
    let _: Option<Arc<dyn ModuleStateStore>> = None;
    let _: Option<Arc<dyn RateLimitStore>> = None;
    let _: Option<Arc<dyn CacheStore>> = None;
    let _: Option<Arc<dyn DiscordCache>> = None;
    let _: Option<Arc<dyn RenderCache>> = None;
    let _: Option<Arc<dyn AssetStore>> = None;
    let _: Option<Arc<dyn LocaleSource>> = None;
    let _: Option<Arc<dyn GuildAccessList>> = None;
    let _: Option<Arc<dyn Renderer>> = None;
}

/// A toy `CacheStore`: one map, no TTL, no bound.
#[derive(Default)]
struct ToyCache(Mutex<HashMap<String, Bytes>>);

impl ToyCache {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Bytes>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CacheStore for ToyCache {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        Box::pin(ready(Ok(self.map().get(key).cloned())))
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.map().insert(key.to_owned(), value);
        Box::pin(ready(Ok(())))
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        let mut map = self.map();
        let inserted = !map.contains_key(key);
        if inserted {
            map.insert(key.to_owned(), value);
        }
        Box::pin(ready(Ok(inserted)))
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.map().remove(key);
        Box::pin(ready(Ok(())))
    }

    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        let mut map = self.map();
        let result = match map.get(key) {
            None => Ok(by),
            Some(raw) => <[u8; 8]>::try_from(raw.as_ref())
                .map(|raw| u64::from_be_bytes(raw).saturating_add(by))
                .map_err(|_| PortError::invalid_value(key, "not an 8-byte counter")),
        };
        if let Ok(next) = result {
            map.insert(key.to_owned(), Bytes::copy_from_slice(&next.to_be_bytes()));
        }
        Box::pin(ready(result))
    }
}

/// A caller that only knows the trait.
fn exercise(store: &dyn CacheStore) -> Result<(), PortError> {
    now(store.put("k", Bytes::from_static(b"v"), None))?;
    assert_eq!(now(store.get("k"))?, Some(Bytes::from_static(b"v")));
    assert!(!now(store.put_if_absent(
        "k",
        Bytes::from_static(b"w"),
        None
    ))?);
    now(store.delete("k"))?;
    assert_eq!(now(store.get("k"))?, None);
    assert_eq!(now(store.incr_with_ttl("n", 5, None))?, 5);
    assert_eq!(now(store.incr_with_ttl("n", 5, None))?, 10);
    Ok(())
}

#[test]
fn cache_store_swaps_without_caller_change() {
    let store: Arc<dyn CacheStore> = Arc::new(ToyCache::default());
    exercise(&*store).unwrap();
}

struct Stub;

fn none<'a, T: Send + 'a>() -> BoxFuture<'a, Result<Option<T>, PortError>> {
    Box::pin(ready(Ok(None)))
}

fn unit<'a>() -> BoxFuture<'a, Result<(), PortError>> {
    Box::pin(ready(Ok(())))
}

impl RefStore for Stub {
    fn put_if_absent<'a>(
        &'a self,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        Box::pin(ready(Ok(true)))
    }
    fn get<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn take<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn delete<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
    fn touch<'a>(
        &'a self,
        _key: &'a str,
        _ttl: Duration,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        Box::pin(ready(Ok(true)))
    }
}

impl SessionStore for Stub {
    fn get<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn put<'a>(
        &'a self,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
    fn delete<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
}

impl ModuleStateStore for Stub {
    fn get<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn put<'a>(
        &'a self,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
    fn version(&self) -> BoxFuture<'_, Result<u64, PortError>> {
        Box::pin(ready(Ok(0)))
    }
}

impl RateLimitStore for Stub {
    fn incr_with_ttl<'a>(
        &'a self,
        _key: &'a str,
        by: u64,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        Box::pin(ready(Ok(by)))
    }
    fn get<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn put<'a>(
        &'a self,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
}

impl RenderCache for Stub {
    fn get<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn put<'a>(
        &'a self,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
}

impl AssetStore for Stub {
    fn get<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        none()
    }
    fn put<'a>(
        &'a self,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
}

impl IdentifyQueue for Stub {
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>> {
        Box::pin(ready(Ok(Lease::new(bucket))))
    }
}

impl LocaleSource for Stub {
    fn load<'a>(
        &'a self,
        _locale: &'a str,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>> {
        Box::pin(ready(Ok(vec![])))
    }
}

impl GuildAccessList for Stub {
    fn allows(&self, _guild: Id<GuildMarker>) -> bool {
        true
    }
}

impl DiscordCache for Stub {
    fn update<'a>(&'a self, _event: &'a Event) -> BoxFuture<'a, Result<(), PortError>> {
        unit()
    }
    fn guild(&self, _guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>> {
        none()
    }
    fn channel(
        &self,
        _channel: Id<ChannelMarker>,
    ) -> BoxFuture<'_, Result<Option<Channel>, PortError>> {
        none()
    }
    fn role(&self, _role: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>> {
        none()
    }
    fn member(
        &self,
        _guild: Id<GuildMarker>,
        _user: Id<UserMarker>,
    ) -> BoxFuture<'_, Result<Option<Member>, PortError>> {
        none()
    }
    fn user(&self, _user: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>> {
        none()
    }
    fn message(
        &self,
        _message: Id<MessageMarker>,
    ) -> BoxFuture<'_, Result<Option<Message>, PortError>> {
        none()
    }
    fn channel_ids(
        &self,
        _guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>> {
        none()
    }
    fn role_ids(
        &self,
        _guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>> {
        none()
    }
    fn member_ids(
        &self,
        _guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>> {
        none()
    }
}

impl Renderer for Stub {
    fn render(&self, _job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        Box::pin(ready(Err(RenderError::QueueFull)))
    }
    fn register(&self, _templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

#[test]
fn every_signature_is_callable_through_arc_dyn() {
    let v = Bytes::from_static(b"v");
    let ttl = Some(Duration::from_secs(1));

    let refs: Arc<dyn RefStore> = Arc::new(Stub);
    let inserted: bool = now(refs.put_if_absent("k", v.clone(), ttl)).unwrap();
    assert!(inserted);
    let got: Option<Bytes> = now(refs.get("k")).unwrap();
    assert_eq!(got, None);
    let taken: Option<Bytes> = now(refs.take("k")).unwrap();
    assert_eq!(taken, None);
    now(refs.delete("k")).unwrap();
    let touched: bool = now(refs.touch("k", Duration::from_secs(1))).unwrap();
    assert!(touched);

    let sessions: Arc<dyn SessionStore> = Arc::new(Stub);
    assert_eq!(now(sessions.get("k")).unwrap(), None);
    now(sessions.put("k", v.clone(), None)).unwrap();
    now(sessions.delete("k")).unwrap();

    let modules: Arc<dyn ModuleStateStore> = Arc::new(Stub);
    assert_eq!(now(modules.get("k")).unwrap(), None);
    now(modules.put("k", v.clone(), None)).unwrap();
    let version: u64 = now(modules.version()).unwrap();
    assert_eq!(version, 0);

    let limits: Arc<dyn RateLimitStore> = Arc::new(Stub);
    let n: u64 = now(limits.incr_with_ttl("k", 3, ttl)).unwrap();
    assert_eq!(n, 3);
    assert_eq!(now(limits.get("k")).unwrap(), None);
    now(limits.put("k", v.clone(), ttl)).unwrap();

    let renders: Arc<dyn RenderCache> = Arc::new(Stub);
    assert_eq!(now(renders.get("k")).unwrap(), None);
    now(renders.put("k", v.clone(), ttl)).unwrap();

    let assets: Arc<dyn AssetStore> = Arc::new(Stub);
    assert_eq!(now(assets.get("k")).unwrap(), None);
    now(assets.put("k", v, ttl)).unwrap();

    let identify: Arc<dyn IdentifyQueue> = Arc::new(Stub);
    let lease: Lease = now(identify.acquire(4)).unwrap();
    assert_eq!(lease.bucket(), 4);

    let locales: Arc<dyn LocaleSource> = Arc::new(Stub);
    let overlay: Vec<(String, String)> = now(locales.load("en-US")).unwrap();
    assert!(overlay.is_empty());

    let access: Arc<dyn GuildAccessList> = Arc::new(Stub);
    assert!(access.allows(Id::new(1)));

    let cache: Arc<dyn DiscordCache> = Arc::new(Stub);
    now(cache.update(&Event::GatewayReconnect)).unwrap();
    let guild: Option<Guild> = now(cache.guild(Id::new(1))).unwrap();
    assert!(guild.is_none());
    let channel: Option<Channel> = now(cache.channel(Id::new(1))).unwrap();
    assert!(channel.is_none());
    let role: Option<Role> = now(cache.role(Id::new(1))).unwrap();
    assert!(role.is_none());
    let member: Option<Member> = now(cache.member(Id::new(1), Id::new(2))).unwrap();
    assert!(member.is_none());
    let user: Option<User> = now(cache.user(Id::new(1))).unwrap();
    assert!(user.is_none());
    let message: Option<Message> = now(cache.message(Id::new(1))).unwrap();
    assert!(message.is_none());
    let channels: Option<Vec<Id<ChannelMarker>>> = now(cache.channel_ids(Id::new(1))).unwrap();
    assert!(channels.is_none());
    let roles: Option<Vec<Id<RoleMarker>>> = now(cache.role_ids(Id::new(1))).unwrap();
    assert!(roles.is_none());
    let members: Option<Vec<Id<UserMarker>>> = now(cache.member_ids(Id::new(1))).unwrap();
    assert!(members.is_none());

    let renderer: Arc<dyn Renderer> = Arc::new(Stub);
    renderer.register(&[]).unwrap();
    let job = RenderJob {
        template_id: "t",
        template_version: 1,
        data: Bytes::new(),
        size: noctis_ports::Size::new(1, 1),
        format: noctis_ports::Format::Png,
        animation: None,
        upload_budget: RenderJob::DEFAULT_UPLOAD_BUDGET,
    };
    assert!(matches!(
        now(renderer.render(job)),
        Err(RenderError::QueueFull)
    ));
}

#[test]
fn lease_echoes_its_bucket() {
    assert_eq!(Lease::new(0).bucket(), 0);
    assert_eq!(Lease::new(u32::MAX).bucket(), u32::MAX);
    assert_eq!(Lease::new(7), Lease::new(7));
}

#[test]
fn port_error_constructors() {
    let failing: Result<(), std::io::Error> = Err(std::io::Error::other("x"));
    let err = failing.map_err(PortError::backend).unwrap_err();
    assert!(matches!(err, PortError::Backend(_)));
    assert!(err.source().is_some());

    let err = PortError::backend("text");
    assert!(matches!(err, PortError::Backend(_)));
    assert!(err.source().is_some());

    let err = PortError::invalid_value("k", "why");
    assert!(matches!(&err, PortError::InvalidValue { key, why } if key == "k" && why == "why"));
    let shown = err.to_string();
    assert!(shown.contains('k') && shown.contains("why"), "{shown}");

    let err = PortError::unavailable(String::from("down"));
    assert!(matches!(&err, PortError::Unavailable { why } if why == "down"));
}
