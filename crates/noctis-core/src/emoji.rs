//! Declared application emoji sets (FR-82, AD-33): the [`AppEmojis`] trait
//! `#[derive(AppEmojis)]` implements, the per-variant [`EmojiDecl`], the
//! Template-drawn [`EmojiArt`], and the App's folded [`EmojiDeclarations`].
//!
//! A set is data: the derive checks names and files at `cargo check`
//! (EMO-02..04) and embeds the files; [`AppBuilder::emojis`] registers a
//! set and `App::build` folds every set once (EMO-01, EMO-03 across sets).
//! Nothing here does I/O.
//!
//! [`AppBuilder::emojis`]: crate::AppBuilder::emojis

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Duration;

use arc_swap::ArcSwap;
use twilight_http::routing::Route;
use twilight_model::id::Id;
use twilight_model::id::marker::EmojiMarker;

use noctis_ports::{RenderJob, Template, TemplateDescriptor, ValueError};
use noctis_rules::Limits;
pub use noctis_rules::emoji::EmojiFormat;

use crate::BuildViolation;
use crate::publish::{PublishError, parse_application, send};

/// A declared set of application emojis: a fieldless enum with
/// `#[derive(AppEmojis)]`, one variant per icon.
///
/// Implement it with the derive, which checks every name and file at
/// `cargo check`; a hand-written impl skips those checks (the App still
/// checks names, uniqueness and the count at build).
pub trait AppEmojis: Copy + Send + Sync + 'static {
    /// One declaration per variant, in declaration order.
    const DECLARATIONS: &'static [EmojiDecl];

    /// This variant's declaration.
    fn declaration(self) -> &'static EmojiDecl;
}

/// Which variant of an icon to draw or look up: the coloured one or the
/// `<name>_white` one (`#[emoji(white)]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmojiTone {
    /// The coloured variant, `<name>`.
    Colour,
    /// The white variant, `<name>_white`, for `Primary`, `Success` and
    /// `Danger` buttons.
    White,
}

/// Where the image of one declared emoji comes from.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum EmojiImage {
    /// A file embedded with `include_bytes!`, already checked (EMO-02,
    /// EMO-04).
    File {
        /// The file's bytes.
        bytes: &'static [u8],
        /// The sniffed format; GIF is animated.
        format: EmojiFormat,
    },
    /// Drawn at Publish by the set's `art` function (`#[emoji(render)]`).
    Render(fn(EmojiTone) -> EmojiArt),
}

impl EmojiImage {
    /// Whether the emoji is animated, when it is known before rendering:
    /// `Some(true)` for a GIF file, `Some(false)` for a PNG or JPEG file,
    /// `None` for a rendered one.
    pub const fn animated(&self) -> Option<bool> {
        match self {
            Self::File { format, .. } => Some(format.is_animated()),
            Self::Render(_) => None,
        }
    }
}

impl fmt::Debug for EmojiImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { bytes, format } => f
                .debug_struct("File")
                .field("format", format)
                .field("len", &bytes.len())
                .finish(),
            Self::Render(_) => f.write_str("Render"),
        }
    }
}

/// One variant of an [`AppEmojis`] set: its name, its image and, with
/// `#[emoji(white)]`, its white variant.
#[derive(Clone, Copy, Debug)]
pub struct EmojiDecl {
    name: &'static str,
    image: EmojiImage,
    white: Option<(&'static str, EmojiImage)>,
}

impl EmojiDecl {
    /// A declaration without a white variant; what the derive emits.
    #[doc(hidden)]
    pub const fn __new(name: &'static str, image: EmojiImage) -> Self {
        Self {
            name,
            image,
            white: None,
        }
    }

    /// Adds the white variant `white_name`; what the derive emits.
    #[doc(hidden)]
    #[must_use]
    pub const fn __white(mut self, white_name: &'static str, image: EmojiImage) -> Self {
        self.white = Some((white_name, image));
        self
    }

    /// The coloured variant's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The coloured variant's image.
    pub const fn image(&self) -> &EmojiImage {
        &self.image
    }

    /// The white variant's name (`<name>_white`), if declared.
    pub const fn white_name(&self) -> Option<&'static str> {
        match &self.white {
            Some((name, _)) => Some(name),
            None => None,
        }
    }

    /// The white variant's image, if declared.
    pub const fn white_image(&self) -> Option<&EmojiImage> {
        match &self.white {
            Some((_, image)) => Some(image),
            None => None,
        }
    }

    /// The name of the variant in `tone`; `None` for [`EmojiTone::White`]
    /// without a white variant.
    pub const fn name_in(&self, tone: EmojiTone) -> Option<&'static str> {
        match tone {
            EmojiTone::Colour => Some(self.name),
            EmojiTone::White => self.white_name(),
        }
    }
}

/// The art of a `#[emoji(render)]` declaration: a Template drawn through
/// the App's Renderer at Publish.
///
/// The set's `art` function (`#[emojis(art = path::to::fn)]`, a
/// `fn(Self, EmojiTone) -> EmojiArt`) returns it for every rendered
/// variant and tone.
///
/// ```
/// use noctis_core::emoji::{EmojiArt, EmojiTone};
/// # use noctis_ports::{AnimationSpec, ErasedView, Format, Size, Template};
/// # #[derive(serde::Serialize, serde::Deserialize)]
/// # struct Glyph { white: bool }
/// # impl Template for Glyph {
/// #     const ID: &'static str = "emoji.glyph";
/// #     const VERSION: u32 = 1;
/// #     const SIZE: Size = Size::new(128, 128);
/// #     const FORMAT: Format = Format::Png;
/// #     const ANIMATION: Option<AnimationSpec> = None;
/// #     fn view() -> ErasedView { ErasedView::new(()) }
/// # }
///
/// let art = EmojiArt::template(Glyph { white: true });
/// assert_eq!(art.template_id(), "emoji.glyph");
/// ```
pub struct EmojiArt {
    pub(crate) descriptor: TemplateDescriptor,
    pub(crate) job: Result<RenderJob, ValueError>,
}

impl EmojiArt {
    /// The art drawn by `template`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "an art function builds the value inline: EmojiArt::template(Glyph { .. })"
    )]
    pub fn template<T: Template>(template: T) -> Self {
        Self {
            descriptor: TemplateDescriptor::of::<T>(),
            job: RenderJob::of(&template),
        }
    }

    /// The Template's id.
    pub fn template_id(&self) -> &'static str {
        self.descriptor.id
    }
}

impl fmt::Debug for EmojiArt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmojiArt")
            .field("template", &self.template_id())
            .finish_non_exhaustive()
    }
}

/// How Publish treats the application emojis it finds
/// ([`AppBuilder::emoji_sync`](crate::AppBuilder::emoji_sync)).
///
/// Publish always creates every declared name the application lacks and
/// never touches anything else by default: an emoji whose declared art
/// changed is reported as `changed` and kept, an emoji the code does not
/// declare is left alone.
///
/// ```
/// use noctis_core::emoji::EmojiSync;
///
/// let sync = EmojiSync::new().replace_changed(true);
/// assert!(sync.replaces_changed());
/// assert!(!sync.deletes_undeclared());
/// assert_eq!(EmojiSync::default(), EmojiSync::new());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmojiSync {
    replace_changed: bool,
    delete_undeclared: bool,
}

impl EmojiSync {
    /// The default policy: create the missing names, replace and delete
    /// nothing.
    pub const fn new() -> Self {
        Self {
            replace_changed: false,
            delete_undeclared: false,
        }
    }

    /// Deletes and re-creates an emoji whose declared art changed (found by
    /// the digest Publish keeps in the App's `Kv`). The emoji gets a new
    /// id, so messages already sent show its old `<:name:id>` token as
    /// plain text.
    #[must_use]
    pub const fn replace_changed(self, replace: bool) -> Self {
        Self {
            replace_changed: replace,
            ..self
        }
    }

    /// Deletes the application's emojis that no registered set declares.
    /// Off by default: emojis uploaded by hand are kept.
    #[must_use]
    pub const fn delete_undeclared(self, delete: bool) -> Self {
        Self {
            delete_undeclared: delete,
            ..self
        }
    }

    /// Whether changed emojis are replaced.
    pub const fn replaces_changed(&self) -> bool {
        self.replace_changed
    }

    /// Whether undeclared emojis are deleted.
    pub const fn deletes_undeclared(&self) -> bool {
        self.delete_undeclared
    }
}

/// One declared emoji of an App: a name, its image and its tone, from one
/// registered set.
#[derive(Clone, Copy, Debug)]
pub struct DeclaredEmoji {
    name: &'static str,
    image: EmojiImage,
    tone: EmojiTone,
    set: &'static str,
}

impl DeclaredEmoji {
    /// The emoji's name on Discord.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Its image.
    pub const fn image(&self) -> &EmojiImage {
        &self.image
    }

    /// Whether it is a coloured or a `_white` variant.
    pub const fn tone(&self) -> EmojiTone {
        self.tone
    }

    /// The type name of the set that declares it.
    pub const fn set(&self) -> &'static str {
        self.set
    }
}

/// Every emoji an App declares, folded once at `App::build` from the sets
/// of [`AppBuilder::emojis`](crate::AppBuilder::emojis), in registration
/// order (each variant, then its white variant). Empty without a set, and
/// then the App makes no emoji request.
#[derive(Clone, Debug, Default)]
pub struct EmojiDeclarations {
    emojis: Vec<DeclaredEmoji>,
}

impl EmojiDeclarations {
    /// The number of declared names, `_white` variants included.
    pub fn len(&self) -> usize {
        self.emojis.len()
    }

    /// Whether no set is registered (or every set is empty).
    pub fn is_empty(&self) -> bool {
        self.emojis.is_empty()
    }

    /// Every declared emoji, in fold order.
    pub fn all(&self) -> &[DeclaredEmoji] {
        &self.emojis
    }

    /// The declared emoji named `name`.
    pub fn get(&self, name: &str) -> Option<&DeclaredEmoji> {
        self.emojis.iter().find(|e| e.name == name)
    }

    /// The first declared emoji drawn by a Template (`#[emoji(render)]`).
    pub(crate) fn first_rendered(&self) -> Option<&DeclaredEmoji> {
        self.emojis
            .iter()
            .find(|e| matches!(e.image, EmojiImage::Render(_)))
    }

    /// The Templates of every drawn emoji, one per Template id, in fold
    /// order: `App::build` registers them with the Renderer.
    pub(crate) fn art_templates(&self) -> Vec<TemplateDescriptor> {
        let mut templates: Vec<TemplateDescriptor> = Vec::new();
        for emoji in &self.emojis {
            if let EmojiImage::Render(art) = emoji.image {
                let descriptor = art(emoji.tone).descriptor;
                if templates.iter().all(|t| t.id != descriptor.id) {
                    templates.push(descriptor);
                }
            }
        }
        templates
    }
}

/// One set registered with `AppBuilder::emojis`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EmojiSet {
    pub(crate) id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) declarations: &'static [EmojiDecl],
}

impl EmojiSet {
    pub(crate) fn of<T: AppEmojis>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            declarations: T::DECLARATIONS,
        }
    }
}

/// Folds `sets` (one per type, in registration order) into the App's
/// declarations: a bad name or a name two sets declare is an EMO-03
/// violation, more than `limits.application_emojis` names an EMO-01 one.
pub(crate) fn fold(sets: &[EmojiSet], limits: &Limits) -> (EmojiDeclarations, Vec<BuildViolation>) {
    let mut emojis: Vec<DeclaredEmoji> = Vec::new();
    let mut violations = Vec::new();
    for set in sets {
        for decl in set.declarations {
            let mut push = |name: &'static str, image: EmojiImage, tone: EmojiTone| {
                if let Err(violation) = noctis_rules::emoji::validate_emoji_name(name) {
                    violations.push(BuildViolation::InvalidEmoji {
                        set: set.type_name,
                        violation: *violation,
                    });
                } else if let Some(first) = emojis.iter().find(|e| e.name == name) {
                    violations.push(BuildViolation::DuplicateEmoji {
                        name,
                        first_set: first.set,
                        second_set: set.type_name,
                    });
                } else {
                    emojis.push(DeclaredEmoji {
                        name,
                        image,
                        tone,
                        set: set.type_name,
                    });
                }
            };
            push(decl.name, decl.image, EmojiTone::Colour);
            if let Some((white, image)) = decl.white {
                push(white, image, EmojiTone::White);
            }
        }
    }
    if emojis.len() > limits.application_emojis {
        violations.push(BuildViolation::TooManyEmojis {
            count: emojis.len(),
            limit: limits.application_emojis,
        });
    }
    (EmojiDeclarations { emojis }, violations)
}

/// The re-list interval while declared names are missing
/// ([`AppBuilder::emoji_reload_interval`](crate::AppBuilder::emoji_reload_interval)).
pub(crate) const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(1);
/// The first retry delay after a failed list; it doubles up to the reload
/// interval (at most [`MAX_RETRY`]).
const FIRST_RETRY: Duration = Duration::from_secs(1);
/// The longest retry delay after failed lists.
const MAX_RETRY: Duration = Duration::from_mins(1);

/// Why listing the application's emojis failed
/// ([`Emojis::reload`]). No message contains the token.
///
/// ```
/// use noctis_core::emoji::EmojiListError;
///
/// assert!(EmojiListError::MissingToken.to_string().contains("NOCTIS_TOKEN"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmojiListError {
    /// The App has no token; nothing was sent.
    #[error(
        "listing the application emojis needs a bot token: set NOCTIS_TOKEN or call \
         `AppBuilder::token`"
    )]
    MissingToken,
    /// The handle belongs to a detached test context, which has no App.
    #[error("this context has no App, so there are no application emojis to list")]
    Detached,
    /// Discord answered with an error status.
    #[error("Discord rejected `{route}` with HTTP {status}: check the token and the application")]
    Rejected {
        /// The method and path, e.g. `GET /applications/7/emojis`.
        route: String,
        /// The HTTP status.
        status: u16,
        /// Discord's JSON error code, when the body has one.
        code: Option<u32>,
    },
    /// The request did not complete (connection, timeout, unreadable body).
    #[error("`{route}` did not complete (network error or timeout); it is retried")]
    Transport {
        /// The method and path.
        route: String,
    },
    /// Discord's answer is not what the route returns.
    #[error("`{route}` returned an unexpected body ({reason})")]
    InvalidResponse {
        /// The method and path.
        route: String,
        /// What is wrong with it.
        reason: &'static str,
    },
}

impl EmojiListError {
    /// The list error of a failed request (`send` answers only these).
    fn from_publish(error: PublishError) -> Self {
        match error {
            PublishError::MissingToken => Self::MissingToken,
            PublishError::Rejected {
                route,
                status,
                code,
            } => Self::Rejected {
                route,
                status,
                code,
            },
            PublishError::InvalidResponse { route, reason } => {
                Self::InvalidResponse { route, reason }
            }
            PublishError::Transport { route } => Self::Transport { route },
            // `send` returns none of the others.
            _ => Self::Transport {
                route: String::from("GET /applications"),
            },
        }
    }
}

/// One listed application emoji: its id and whether it is animated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) id: Id<EmojiMarker>,
    animated: bool,
}

/// Where a table's list comes from.
enum Source {
    /// A detached test context: nothing to list.
    Detached,
    /// The App's one HTTP client.
    Discord {
        http: Arc<twilight_http::Client>,
        has_token: bool,
    },
}

/// The App's emoji table: its declarations and the name → (id, animated)
/// entries of the last successful list.
struct EmojiTable {
    declarations: Arc<EmojiDeclarations>,
    /// Declared names only; replaced whole by each list.
    entries: ArcSwap<HashMap<&'static str, Listed>>,
    source: Source,
    /// The application id, read once from `GET /applications/@me`.
    application: OnceLock<u64>,
    /// Seeded by the testkit: lists are skipped.
    fixed: AtomicBool,
    /// Whether a list has succeeded (or the table was seeded).
    loaded: AtomicBool,
    /// Serializes lists.
    listing: tokio::sync::Mutex<()>,
    /// What Publish replaces and deletes (the default when unset).
    sync: OnceLock<EmojiSync>,
    /// The missing names the last `warn` named.
    warned: Mutex<Option<Vec<&'static str>>>,
    interval: Duration,
}

/// The App's application emoji table: typed lookups of the declared
/// emojis (FR-82, AD-33), from `ctx.emojis()` in every context or
/// [`App::emojis`](crate::App::emojis).
///
/// `App::run` lists the application's emojis once at start
/// (`GET /applications/{id}/emojis`) and builds the name → id table before
/// any handler runs. A failed list never stops the start: the table stays
/// empty and the list is retried with backoff. While a declared name is
/// missing it is re-listed every
/// [`emoji_reload_interval`](crate::AppBuilder::emoji_reload_interval)
/// (60 s), and after every Publish of this process; [`reload`](Self::reload)
/// forces it. Each App has its own table.
///
/// Lookups never do I/O and allocate nothing: a missing emoji is `None` (a
/// [`RouteButton::emoji`](noctis_ui::RouteButton::emoji) given `None` shows
/// none) and an empty [`text`](Self::text) token. Cheap to clone (one
/// `Arc`).
///
/// ```
/// use noctis_core::emoji::{AppEmojis, EmojiDecl, EmojiFormat, EmojiImage};
/// use noctis_ui::ButtonStyle;
///
/// // What `#[derive(AppEmojis)]` implements; derive it instead.
/// #[derive(Clone, Copy)]
/// enum Icons { Check }
/// const PNG: EmojiImage = EmojiImage::File { bytes: b"\x89PNG", format: EmojiFormat::Png };
/// impl AppEmojis for Icons {
///     const DECLARATIONS: &'static [EmojiDecl] =
///         &[EmojiDecl::__new("check", PNG).__white("check_white", PNG)];
///     fn declaration(self) -> &'static EmojiDecl { &Self::DECLARATIONS[0] }
/// }
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
/// let app = noctis_core::App::builder().emojis::<Icons>().build().unwrap();
/// let emojis = app.emojis();
/// // Not listed yet (the App is not running): every lookup is empty.
/// assert_eq!(emojis.get(Icons::Check), None);
/// assert_eq!(emojis.on(ButtonStyle::Primary, Icons::Check), None);
/// assert_eq!(format!("Done {}", emojis.text(Icons::Check)), "Done ");
/// # }
/// ```
#[derive(Clone)]
pub struct Emojis(Arc<EmojiTable>);

impl Emojis {
    /// The table of an App built with `declarations`.
    pub(crate) fn new(
        declarations: EmojiDeclarations,
        http: Arc<twilight_http::Client>,
        has_token: bool,
        interval: Duration,
        sync: EmojiSync,
    ) -> Self {
        let table = Self::with_source(declarations, Source::Discord { http, has_token }, interval);
        table.with_sync(sync)
    }

    fn with_source(declarations: EmojiDeclarations, source: Source, interval: Duration) -> Self {
        Self(Arc::new(EmojiTable {
            declarations: Arc::new(declarations),
            entries: ArcSwap::from_pointee(HashMap::new()),
            source,
            application: OnceLock::new(),
            fixed: AtomicBool::new(false),
            loaded: AtomicBool::new(false),
            listing: tokio::sync::Mutex::new(()),
            sync: OnceLock::new(),
            warned: Mutex::new(None),
            interval,
        }))
    }

    /// The empty table of the detached test contexts.
    pub(crate) fn detached() -> &'static Self {
        static DETACHED: LazyLock<Emojis> = LazyLock::new(|| {
            Emojis::with_source(
                EmojiDeclarations::default(),
                Source::Detached,
                DEFAULT_RELOAD_INTERVAL,
            )
        });
        &DETACHED
    }

    /// Every emoji the App declares.
    pub fn declarations(&self) -> &EmojiDeclarations {
        &self.0.declarations
    }

    /// The coloured variant of `icon`; `None` until a list found it.
    pub fn get<E: AppEmojis>(&self, icon: E) -> Option<noctis_ui::Emoji> {
        self.named(icon.declaration().name())
    }

    /// The white variant of `icon` (`#[emoji(white)]`); `None` when it
    /// declares none or a list has not found it.
    pub fn white<E: AppEmojis>(&self, icon: E) -> Option<noctis_ui::Emoji> {
        self.named(icon.declaration().white_name()?)
    }

    /// The variant of `icon` for a button of `style`: the white one on
    /// `Primary`, `Success` and `Danger` when `icon` declares one (the
    /// coloured one while the white one is missing), else the coloured
    /// one; the coloured one on `Secondary`.
    pub fn on<E: AppEmojis>(
        &self,
        style: noctis_ui::ButtonStyle,
        icon: E,
    ) -> Option<noctis_ui::Emoji> {
        use noctis_ui::ButtonStyle;
        match style {
            ButtonStyle::Primary | ButtonStyle::Success | ButtonStyle::Danger => {
                self.white(icon).or_else(|| self.get(icon))
            }
            ButtonStyle::Secondary => self.get(icon),
        }
    }

    /// The coloured variant of `icon` as a message token: `Display` writes
    /// `<:name:id>` (`<a:name:id>` when animated), and nothing while the
    /// emoji is missing.
    pub fn text<E: AppEmojis>(&self, icon: E) -> EmojiText {
        let name = icon.declaration().name();
        EmojiText(self.listed(name).map(|listed| (name, listed)))
    }

    /// The id of the declared emoji `name` (a coloured or `_white` name);
    /// `None` until a list found it.
    pub fn id(&self, name: &str) -> Option<Id<EmojiMarker>> {
        self.listed(name).map(|listed| listed.id)
    }

    /// Whether a list has succeeded since the App started.
    pub fn is_loaded(&self) -> bool {
        self.0.loaded.load(Ordering::Acquire)
    }

    /// The declared names the last list did not find, in declaration
    /// order (every declared name before the first list).
    pub fn missing(&self) -> Vec<&'static str> {
        let entries = self.0.entries.load();
        self.0
            .declarations
            .all()
            .iter()
            .map(DeclaredEmoji::name)
            .filter(|name| !entries.contains_key(name))
            .collect()
    }

    /// Lists the application's emojis now and replaces the table; a
    /// failure keeps the current table. Concurrent calls list one after
    /// the other. An App without emoji sets sends nothing.
    ///
    /// # Errors
    ///
    /// [`EmojiListError`]: no token, a detached context, or a failed
    /// request.
    pub async fn reload(&self) -> Result<(), EmojiListError> {
        let table = &*self.0;
        let Source::Discord { http, has_token } = &table.source else {
            return Err(EmojiListError::Detached);
        };
        if table.declarations.is_empty() || table.fixed.load(Ordering::Acquire) {
            return Ok(());
        }
        if !has_token {
            return Err(EmojiListError::MissingToken);
        }
        let _listing = table.listing.lock().await;
        let application = if let Some(&id) = table.application.get() {
            id
        } else {
            let label = "GET /applications/@me";
            let body = send(http, &Route::GetCurrentUserApplicationInfo, label, None)
                .await
                .map_err(EmojiListError::from_publish)?;
            let id = parse_application(&body)
                .map_err(|reason| EmojiListError::InvalidResponse {
                    route: label.to_owned(),
                    reason,
                })?
                .id;
            *table.application.get_or_init(|| id)
        };
        let label = format!("GET /applications/{application}/emojis");
        let body = send(
            http,
            &Route::GetApplicationEmojis {
                application_id: application,
            },
            &label,
            None,
        )
        .await
        .map_err(EmojiListError::from_publish)?;
        let listed = parse_emoji_list(&body).map_err(|reason| EmojiListError::InvalidResponse {
            route: label,
            reason,
        })?;
        self.install(&listed);
        Ok(())
    }

    /// Replaces the entries with the declared names of `listed` and logs
    /// the missing names when they changed.
    fn install(&self, listed: &HashMap<String, Listed>) {
        let table = &*self.0;
        let entries: HashMap<&'static str, Listed> = table
            .declarations
            .all()
            .iter()
            .filter_map(|e| listed.get(e.name()).map(|&l| (e.name(), l)))
            .collect();
        table.entries.store(Arc::new(entries));
        table.loaded.store(true, Ordering::Release);
        self.log_missing();
    }

    /// One `warn` naming the declared names the application lacks, when
    /// they differ from the last one logged.
    fn log_missing(&self) {
        let missing = self.missing();
        let mut warned = self
            .0
            .warned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if warned.as_deref() == Some(missing.as_slice()) {
            return;
        }
        if missing.is_empty() {
            tracing::info!(target: "noctis::emoji", "every declared application emoji is listed");
        } else {
            tracing::warn!(
                target: "noctis::emoji",
                missing = %missing.join(", "),
                "the application lacks {} declared emoji(s): run Publish (--publish) to upload them; \
                 lookups of these return None until then",
                missing.len()
            );
        }
        *warned = Some(missing);
    }

    /// Seeds the table without a list (the testkit): every declared name
    /// `keep` accepts, with the id `id_of(name)`. Later lists are skipped.
    pub(crate) fn seed(
        &self,
        keep: &dyn Fn(&str) -> bool,
        id_of: &dyn Fn(&str) -> Id<EmojiMarker>,
    ) {
        let table = &*self.0;
        table.fixed.store(true, Ordering::Release);
        let entries: HashMap<&'static str, Listed> = table
            .declarations
            .all()
            .iter()
            .filter(|e| keep(e.name()))
            .map(|e| {
                let listed = Listed {
                    id: id_of(e.name()),
                    animated: e.image().animated().unwrap_or(false),
                };
                (e.name(), listed)
            })
            .collect();
        table.entries.store(Arc::new(entries));
        table.loaded.store(true, Ordering::Release);
    }

    fn listed(&self, name: &str) -> Option<Listed> {
        self.0.entries.load().get(name).copied()
    }

    fn named(&self, name: &'static str) -> Option<noctis_ui::Emoji> {
        self.listed(name)
            .map(|l| noctis_ui::Emoji::custom(l.id, name, l.animated))
    }

    /// The App's sync policy.
    pub(crate) fn sync_policy(&self) -> EmojiSync {
        self.0.sync.get().copied().unwrap_or_default()
    }

    fn with_sync(self, sync: EmojiSync) -> Self {
        let _ = self.0.sync.set(sync);
        self
    }

    /// Remembers the application id a Publish read, so a later list skips
    /// `GET /applications/@me`.
    pub(crate) fn note_application(&self, id: u64) {
        let _ = self.0.application.set(id);
    }

    /// Whether the App declares any emoji (and so lists at start).
    pub(crate) fn has_declarations(&self) -> bool {
        !self.0.declarations.is_empty()
    }

    /// After the first list of a run (`first`), the retries and re-lists: a failure
    /// retries with backoff (1 s doubling to 60 s); a success re-lists
    /// every reload interval while a declared name is missing, and ends
    /// once none is (a Publish re-lists itself).
    pub(crate) async fn keep_listed(self, first: Result<(), EmojiListError>) {
        let mut result = first;
        let mut retry = FIRST_RETRY;
        loop {
            let wait = if let Err(error) = &result {
                tracing::warn!(
                    target: "noctis::emoji",
                    %error,
                    retry_in = ?retry,
                    "could not list the application emojis; lookups return None until a \
                     retry succeeds"
                );
                let wait = retry;
                retry = (retry * 2).min(MAX_RETRY);
                wait
            } else {
                retry = FIRST_RETRY;
                if self.0.interval.is_zero() || self.missing().is_empty() {
                    return;
                }
                self.0.interval
            };
            tokio::time::sleep(wait).await;
            result = self.reload().await;
        }
    }
}

impl fmt::Debug for Emojis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emojis")
            .field("declared", &self.0.declarations.len())
            .field("listed", &self.0.entries.load().len())
            .field("loaded", &self.is_loaded())
            .finish()
    }
}

/// The table of `app`, or the empty detached one.
pub(crate) fn emojis_of(app: Option<&crate::handler::AppShared>) -> &Emojis {
    match app {
        Some(app) => &app.emojis,
        None => Emojis::detached(),
    }
}

/// An application emoji as a message token, from [`Emojis::text`]:
/// `Display` writes `<:name:id>` or `<a:name:id>`, and nothing while the
/// emoji is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmojiText(Option<(&'static str, Listed)>);

impl EmojiText {
    /// Whether the emoji is missing (the token writes nothing).
    pub const fn is_missing(&self) -> bool {
        self.0.is_none()
    }
}

impl fmt::Display for EmojiText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some((name, Listed { id, animated: true })) => write!(f, "<a:{name}:{id}>"),
            Some((
                name,
                Listed {
                    id,
                    animated: false,
                },
            )) => write!(f, "<:{name}:{id}>"),
            None => Ok(()),
        }
    }
}

/// The `name` → (id, animated) of a `GET /applications/{id}/emojis` body
/// (`{"items": [..]}`); items without a name or id are skipped.
pub(crate) fn parse_emoji_list(body: &[u8]) -> Result<HashMap<String, Listed>, &'static str> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| "not JSON")?;
    let items = value
        .get("items")
        .and_then(serde_json::Value::as_array)
        .ok_or("no `items` array")?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?;
            let id = match item.get("id")? {
                serde_json::Value::String(id) => id.parse::<u64>().ok(),
                serde_json::Value::Number(id) => id.as_u64(),
                _ => None,
            }?;
            let animated = item
                .get("animated")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            Some((
                name.to_owned(),
                Listed {
                    id: Id::new_checked(id)?,
                    animated,
                },
            ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const FILE: EmojiImage = EmojiImage::File {
        bytes: PNG,
        format: EmojiFormat::Png,
    };

    #[derive(Clone, Copy)]
    struct A;
    impl AppEmojis for A {
        const DECLARATIONS: &'static [EmojiDecl] = &[
            EmojiDecl::__new("check", FILE).__white("check_white", FILE),
            EmojiDecl::__new("cross", FILE),
        ];
        fn declaration(self) -> &'static EmojiDecl {
            &Self::DECLARATIONS[0]
        }
    }

    #[derive(Clone, Copy)]
    struct B;
    impl AppEmojis for B {
        const DECLARATIONS: &'static [EmojiDecl] =
            &[EmojiDecl::__new("cross", FILE), EmojiDecl::__new("x", FILE)];
        fn declaration(self) -> &'static EmojiDecl {
            &Self::DECLARATIONS[0]
        }
    }

    #[test]
    fn fold_keeps_order_and_white_pairs() {
        let (decls, violations) = fold(&[EmojiSet::of::<A>()], &Limits::default());
        assert!(violations.is_empty());
        let names: Vec<_> = decls.all().iter().map(DeclaredEmoji::name).collect();
        assert_eq!(names, ["check", "check_white", "cross"]);
        assert_eq!(decls.get("check_white").unwrap().tone(), EmojiTone::White);
        assert_eq!(decls.get("check").unwrap().image().animated(), Some(false));
        assert!(decls.get("nope").is_none());
        assert_eq!(decls.len(), 3);
        let decl = A.declaration();
        assert_eq!(decl.name_in(EmojiTone::White), Some("check_white"));
        assert_eq!(A::DECLARATIONS[1].name_in(EmojiTone::White), None);
    }

    #[test]
    fn emo_03_duplicate_and_bad_names_across_sets() {
        let (decls, violations) = fold(
            &[EmojiSet::of::<A>(), EmojiSet::of::<B>()],
            &Limits::default(),
        );
        assert_eq!(decls.len(), 3);
        assert_eq!(violations.len(), 2);
        assert!(matches!(
            violations[0],
            BuildViolation::DuplicateEmoji { name: "cross", .. }
        ));
        assert!(
            violations[0]
                .to_string()
                .starts_with("[EMO-03] emoji `cross` is declared by two sets")
        );
        assert!(matches!(violations[1], BuildViolation::InvalidEmoji { .. }));
        assert!(
            violations[1].to_string().starts_with("[EMO-03]"),
            "{}",
            violations[1]
        );
    }

    #[test]
    fn emo_01_too_many_emojis() {
        let mut limits = Limits::default();
        limits.application_emojis = 2;
        let (_, violations) = fold(&[EmojiSet::of::<A>()], &limits);
        assert_eq!(
            violations,
            [BuildViolation::TooManyEmojis { count: 3, limit: 2 }]
        );
        assert!(violations[0].to_string().starts_with("[EMO-01]"));
    }

    #[test]
    fn empty_fold_declares_nothing() {
        let (decls, violations) = fold(&[], &Limits::default());
        assert!(decls.is_empty() && violations.is_empty());
    }

    const GIF: EmojiImage = EmojiImage::File {
        bytes: b"GIF89a",
        format: EmojiFormat::Gif,
    };

    #[derive(Clone, Copy, Debug)]
    enum Icon {
        Check,
        Cross,
        Spin,
    }
    impl AppEmojis for Icon {
        const DECLARATIONS: &'static [EmojiDecl] = &[
            EmojiDecl::__new("check", FILE).__white("check_white", FILE),
            EmojiDecl::__new("cross", FILE),
            EmojiDecl::__new("spin", GIF),
        ];
        fn declaration(self) -> &'static EmojiDecl {
            &Self::DECLARATIONS[self as usize]
        }
    }

    fn id(name: &str) -> Id<EmojiMarker> {
        Id::new(match name {
            "check" => 10,
            "check_white" => 11,
            "cross" => 20,
            _ => 30,
        })
    }

    /// A detached-source table of [`Icon`] seeded with the names `keep`
    /// accepts.
    fn table(keep: &dyn Fn(&str) -> bool) -> Emojis {
        let (decls, _) = fold(&[EmojiSet::of::<Icon>()], &Limits::default());
        let emojis = Emojis::with_source(decls, Source::Detached, DEFAULT_RELOAD_INTERVAL);
        emojis.seed(keep, &id);
        emojis
    }

    fn custom(id: u64, name: &'static str) -> noctis_ui::Emoji {
        noctis_ui::Emoji::custom(Id::new(id), name, false)
    }

    #[test]
    fn on_picks_the_variant_for_every_button_style() {
        use noctis_ui::ButtonStyle;
        let all = table(&|_| true);
        for style in [
            ButtonStyle::Primary,
            ButtonStyle::Success,
            ButtonStyle::Danger,
        ] {
            assert_eq!(
                all.on(style, Icon::Check),
                Some(custom(11, "check_white")),
                "{style:?}"
            );
            // No white variant declared: the coloured one.
            assert_eq!(
                all.on(style, Icon::Cross),
                Some(custom(20, "cross")),
                "{style:?}"
            );
        }
        assert_eq!(
            all.on(ButtonStyle::Secondary, Icon::Check),
            Some(custom(10, "check"))
        );
        assert_eq!(all.get(Icon::Check), Some(custom(10, "check")));
        assert_eq!(all.white(Icon::Check), Some(custom(11, "check_white")));
        assert_eq!(all.white(Icon::Cross), None);

        // The white variant missing: the coloured one; both missing: None.
        let no_white = table(&|name| name != "check_white");
        assert_eq!(
            no_white.on(ButtonStyle::Primary, Icon::Check),
            Some(custom(10, "check"))
        );
        let none = table(&|_| false);
        assert_eq!(none.on(ButtonStyle::Danger, Icon::Check), None);
        assert_eq!(none.on(ButtonStyle::Secondary, Icon::Check), None);
        assert_eq!(none.missing(), ["check", "check_white", "cross", "spin"]);
        assert!(none.is_loaded());
    }

    #[test]
    fn text_tokens() {
        let all = table(&|_| true);
        assert_eq!(all.text(Icon::Check).to_string(), "<:check:10>");
        assert_eq!(all.text(Icon::Spin).to_string(), "<a:spin:30>");
        assert_eq!(
            all.get(Icon::Spin),
            Some(noctis_ui::Emoji::custom(Id::new(30), "spin", true))
        );
        let none = table(&|name| name == "check");
        assert_eq!(format!("[{}]", none.text(Icon::Cross)), "[]");
        assert!(none.text(Icon::Cross).is_missing());
        assert_eq!(none.id("check"), Some(Id::new(10)));
        assert_eq!(none.missing(), ["check_white", "cross", "spin"]);
    }

    #[test]
    fn detached_table_is_empty() {
        let detached = Emojis::detached();
        assert_eq!(detached.get(Icon::Check), None);
        assert!(!detached.is_loaded());
        assert!(detached.declarations().is_empty());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(detached.reload()),
            Err(EmojiListError::Detached)
        );
    }

    #[test]
    fn parses_the_emoji_list() {
        let body = br#"{"items": [
            {"id": "5", "name": "check", "animated": false},
            {"id": 6, "name": "spin", "animated": true},
            {"id": "0", "name": "zero"},
            {"name": "no_id"}
        ]}"#;
        let listed = parse_emoji_list(body).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed["spin"],
            Listed {
                id: Id::new(6),
                animated: true
            }
        );
        assert_eq!(parse_emoji_list(b"[]"), Err("no `items` array"));
        assert_eq!(parse_emoji_list(b"x"), Err("not JSON"));
    }

    #[test]
    fn debug_hides_bytes() {
        assert_eq!(format!("{FILE:?}"), "File { format: Png, len: 8 }");
    }
}
