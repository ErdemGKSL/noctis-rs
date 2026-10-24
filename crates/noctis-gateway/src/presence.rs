//! The bot's presence ([`Presence`], [`Activity`], [`Status`]) and the
//! IDENTIFY `large_threshold` ([`LargeThreshold`], GW-06).

use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;
use twilight_model::gateway::presence::{
    Activity as TwilightActivity, ActivityType, Status as TwilightStatus,
};

/// The online status a [`Presence`] shows.
///
/// ```
/// use noctis_gateway::Status;
///
/// assert_eq!(Status::default(), Status::Online);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Status {
    /// Online (green; `online`).
    #[default]
    Online,
    /// Idle (yellow; `idle`).
    Idle,
    /// Do Not Disturb (red; `dnd`).
    DoNotDisturb,
    /// Shown as offline while connected (`invisible`).
    Invisible,
}

impl Status {
    /// Twilight's status.
    const fn to_twilight(self) -> TwilightStatus {
        match self {
            Self::Online => TwilightStatus::Online,
            Self::Idle => TwilightStatus::Idle,
            Self::DoNotDisturb => TwilightStatus::DoNotDisturb,
            Self::Invisible => TwilightStatus::Invisible,
        }
    }
}

/// The kinds of activity a bot may show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Custom,
    Competing,
}

impl ActivityKind {
    const fn to_twilight(self) -> ActivityType {
        match self {
            Self::Playing => ActivityType::Playing,
            Self::Streaming => ActivityType::Streaming,
            Self::Listening => ActivityType::Listening,
            Self::Watching => ActivityType::Watching,
            Self::Custom => ActivityType::Custom,
            Self::Competing => ActivityType::Competing,
        }
    }
}

/// The `name` Discord expects on a custom status (activity type 4).
const CUSTOM_STATUS_NAME: &str = "Custom Status";

/// One activity of a [`Presence`]: "Playing x", "Watching x", a custom
/// status, ...
///
/// Only the fields a bot may set are ever sent: `name`, `type`, `state`
/// and `url`. Texts are sent as given; Discord documents no length limit
/// for them.
///
/// ```
/// use noctis_gateway::Activity;
///
/// let watching = Activity::watching("servers").state("in 3 regions");
/// let custom = Activity::custom("☕ brewing");
/// let live = Activity::streaming("live", "https://twitch.tv/x");
/// # let _ = (watching, custom, live);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Activity {
    kind: ActivityKind,
    name: String,
    state: Option<String>,
    url: Option<String>,
}

impl Activity {
    fn new(kind: ActivityKind, name: String) -> Self {
        Self {
            kind,
            name,
            state: None,
            url: None,
        }
    }

    /// "Playing `name`" (type 0).
    pub fn playing(name: impl Into<String>) -> Self {
        Self::new(ActivityKind::Playing, name.into())
    }

    /// "Listening to `name`" (type 2).
    pub fn listening(name: impl Into<String>) -> Self {
        Self::new(ActivityKind::Listening, name.into())
    }

    /// "Watching `name`" (type 3).
    pub fn watching(name: impl Into<String>) -> Self {
        Self::new(ActivityKind::Watching, name.into())
    }

    /// "Competing in `name`" (type 5).
    pub fn competing(name: impl Into<String>) -> Self {
        Self::new(ActivityKind::Competing, name.into())
    }

    /// "Streaming `name`" (type 1) linking to `url` (a Twitch or
    /// `YouTube` URL).
    pub fn streaming(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            ..Self::new(ActivityKind::Streaming, name.into())
        }
    }

    /// A custom status showing `state` (type 4, `name` "Custom Status").
    pub fn custom(state: impl Into<String>) -> Self {
        Self {
            state: Some(state.into()),
            ..Self::new(ActivityKind::Custom, CUSTOM_STATUS_NAME.to_owned())
        }
    }

    /// Sets the activity's `state` line (the text of a custom status).
    #[must_use]
    pub fn state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Twilight's activity, holding only `name`, `type`, `state` and `url`.
    fn to_twilight(&self) -> TwilightActivity {
        TwilightActivity {
            application_id: None,
            assets: None,
            buttons: Vec::new(),
            created_at: None,
            details: None,
            emoji: None,
            flags: None,
            id: None,
            instance: None,
            kind: self.kind.to_twilight(),
            name: self.name.clone(),
            party: None,
            secrets: None,
            state: self.state.clone(),
            timestamps: None,
            url: self.url.clone(),
        }
    }
}

/// The bot's presence: a [`Status`], at most one [`Activity`] and the AFK
/// flag.
///
/// Set the presence every shard identifies with through
/// `AppBuilder::presence`, and change it at run time with
/// [`PresenceHandle::set`](crate::PresenceHandle::set) (every shard) or
/// [`set_shard`](crate::PresenceHandle::set_shard) (one shard).
///
/// ```
/// use noctis_gateway::{Activity, Presence, Status};
///
/// let p = Presence::watching("/help");
/// let away = Presence::idle().activity(Activity::custom("back soon"));
/// let hidden = Presence::new(Status::Invisible).afk(true);
/// # let _ = (p, away, hidden);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Presence {
    status: Status,
    activity: Option<Activity>,
    afk: bool,
}

impl Presence {
    /// A presence with `status`, no activity, not AFK.
    pub const fn new(status: Status) -> Self {
        Self {
            status,
            activity: None,
            afk: false,
        }
    }

    /// [`Status::Online`], no activity.
    pub const fn online() -> Self {
        Self::new(Status::Online)
    }

    /// [`Status::Idle`], no activity.
    pub const fn idle() -> Self {
        Self::new(Status::Idle)
    }

    /// [`Status::DoNotDisturb`], no activity.
    pub const fn dnd() -> Self {
        Self::new(Status::DoNotDisturb)
    }

    /// [`Status::Invisible`], no activity.
    pub const fn invisible() -> Self {
        Self::new(Status::Invisible)
    }

    /// Online, "Playing `name`".
    pub fn playing(name: impl Into<String>) -> Self {
        Self::online().activity(Activity::playing(name))
    }

    /// Online, "Listening to `name`".
    pub fn listening(name: impl Into<String>) -> Self {
        Self::online().activity(Activity::listening(name))
    }

    /// Online, "Watching `name`".
    pub fn watching(name: impl Into<String>) -> Self {
        Self::online().activity(Activity::watching(name))
    }

    /// Online, "Competing in `name`".
    pub fn competing(name: impl Into<String>) -> Self {
        Self::online().activity(Activity::competing(name))
    }

    /// Online, "Streaming `name`" linking to `url`.
    pub fn streaming(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self::online().activity(Activity::streaming(name, url))
    }

    /// Online, a custom status showing `state`.
    pub fn custom(state: impl Into<String>) -> Self {
        Self::online().activity(Activity::custom(state))
    }

    /// Sets the status.
    #[must_use]
    pub const fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Sets the activity (replacing any earlier one).
    #[must_use]
    pub fn activity(mut self, activity: Activity) -> Self {
        self.activity = Some(activity);
        self
    }

    /// Sets the AFK flag.
    #[must_use]
    pub const fn afk(mut self, afk: bool) -> Self {
        self.afk = afk;
        self
    }

    /// Twilight's payload, built by struct literal: its constructor rejects
    /// an empty activity list, which a status-only presence needs.
    pub(crate) fn to_payload(&self) -> UpdatePresencePayload {
        UpdatePresencePayload {
            activities: self.activity.iter().map(Activity::to_twilight).collect(),
            afk: self.afk,
            since: None,
            status: self.status.to_twilight(),
        }
    }
}

/// The IDENTIFY `large_threshold` (GW-06): the member count above which
/// Discord stops sending offline members in `GUILD_CREATE`; 50 by default.
///
/// [`LargeThreshold::new`] takes the value as a const generic and only
/// compiles for 50..=250, Discord's range:
///
/// ```
/// use noctis_gateway::LargeThreshold;
///
/// assert_eq!(LargeThreshold::new::<250>().get(), 250);
/// assert_eq!(LargeThreshold::default().get(), 50);
/// ```
///
/// ```compile_fail
/// let _ = noctis_gateway::LargeThreshold::new::<251>();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LargeThreshold(u16);

impl LargeThreshold {
    /// The threshold `N`, which must be within 50..=250 (a compile error
    /// carrying `[GW-06]` otherwise).
    pub const fn new<const N: u16>() -> Self
    where
        Threshold<N>: ValidLargeThreshold,
    {
        Self(N)
    }

    /// The threshold's value.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Default for LargeThreshold {
    fn default() -> Self {
        Self::new::<50>()
    }
}

/// A `large_threshold` candidate, for [`ValidLargeThreshold`].
#[derive(Debug)]
pub struct Threshold<const N: u16>;

/// Implemented exactly for `Threshold<50>` ..= `Threshold<250>`.
#[diagnostic::on_unimplemented(
    message = "[GW-06] `LargeThreshold::new::<N>()` takes 50..=250 (Discord's `large_threshold` range)",
    label = "outside 50..=250"
)]
pub trait ValidLargeThreshold: crate::sealed::Sealed {}

macro_rules! valid_thresholds {
    ($($n:literal)*) => {
        $(
            impl crate::sealed::Sealed for Threshold<$n> {}
            impl ValidLargeThreshold for Threshold<$n> {}
        )*
    };
}

valid_thresholds!(
    50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79
    80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100 101 102 103 104 105 106 107
    108 109 110 111 112 113 114 115 116 117 118 119 120 121 122 123 124 125 126 127 128 129 130
    131 132 133 134 135 136 137 138 139 140 141 142 143 144 145 146 147 148 149 150 151 152 153
    154 155 156 157 158 159 160 161 162 163 164 165 166 167 168 169 170 171 172 173 174 175 176
    177 178 179 180 181 182 183 184 185 186 187 188 189 190 191 192 193 194 195 196 197 198 199
    200 201 202 203 204 205 206 207 208 209 210 211 212 213 214 215 216 217 218 219 220 221 222
    223 224 225 226 227 228 229 230 231 232 233 234 235 236 237 238 239 240 241 242 243 244 245
    246 247 248 249 250
);

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn wire(presence: &Presence) -> Value {
        serde_json::to_value(presence.to_payload()).unwrap()
    }

    /// The activity's keys other than `created_at` (twilight always writes
    /// it, as `null`).
    fn activity(presence: &Presence) -> Value {
        let mut value = wire(presence)["activities"][0].clone();
        let object = value.as_object_mut().unwrap();
        assert_eq!(object.remove("created_at"), Some(Value::Null));
        value
    }

    /// P1's wire form.
    #[test]
    fn watching_wire_form() {
        let mut value = wire(&Presence::watching("servers"));
        value["activities"][0]
            .as_object_mut()
            .unwrap()
            .remove("created_at");
        assert_eq!(
            value,
            json!({
                "status": "online",
                "afk": false,
                "since": null,
                "activities": [{ "type": 3, "name": "servers" }],
            })
        );
    }

    /// P3: a status-only presence sends an empty activity list.
    #[test]
    fn status_only_has_no_activities() {
        for (presence, status) in [
            (Presence::dnd(), "dnd"),
            (Presence::idle(), "idle"),
            (Presence::invisible(), "invisible"),
            (Presence::online(), "online"),
            (Presence::default(), "online"),
        ] {
            let value = wire(&presence);
            assert_eq!(value["activities"], json!([]), "{status}");
            assert_eq!(value["status"], json!(status));
        }
        assert_eq!(wire(&Presence::dnd().afk(true))["afk"], json!(true));
    }

    /// P4: a custom status keeps its text byte for byte.
    #[test]
    fn custom_status_wire_form() {
        let text = "☕ brewing — 𝔲𝔫𝔦𝔠𝔬𝔡𝔢";
        assert_eq!(
            activity(&Presence::custom(text)),
            json!({ "type": 4, "name": "Custom Status", "state": text })
        );
        let serialized = serde_json::to_string(&Presence::custom(text).to_payload()).unwrap();
        assert!(serialized.contains(text), "{serialized}");
    }

    /// P5, P6: streaming carries the URL; an empty name is sent as given.
    #[test]
    fn streaming_and_empty_name() {
        let streaming =
            Presence::online().activity(Activity::streaming("live", "https://twitch.tv/x"));
        assert_eq!(
            activity(&streaming),
            json!({ "type": 1, "name": "live", "url": "https://twitch.tv/x" })
        );
        assert_eq!(
            activity(&Presence::online().activity(Activity::playing(""))),
            json!({ "type": 0, "name": "" })
        );
        for (presence, kind) in [
            (Presence::playing("a"), 0),
            (Presence::listening("a"), 2),
            (Presence::watching("a"), 3),
            (Presence::competing("a"), 5),
        ] {
            assert_eq!(activity(&presence), json!({ "type": kind, "name": "a" }));
        }
        assert_eq!(
            activity(&Presence::idle().activity(Activity::watching("x").state("y"))),
            json!({ "type": 3, "name": "x", "state": "y" })
        );
        assert_eq!(
            wire(&Presence::playing("a").status(Status::Idle))["status"],
            json!("idle")
        );
    }

    /// L1: both bounds compile; the default is 50.
    #[test]
    fn large_threshold_bounds() {
        assert_eq!(LargeThreshold::new::<50>().get(), 50);
        assert_eq!(LargeThreshold::new::<250>().get(), 250);
        assert_eq!(LargeThreshold::default().get(), 50);
    }
}
