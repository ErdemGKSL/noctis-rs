//! The immutable Registry `build()` folds once from the mounted Module trees
//! (AD-7, AD-9, GW-05), including the merged command Registry
//! ([`commands`]) and the Route map of every component and modal handler
//! (AD-25, CID-04).

pub(crate) mod commands;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use noctis_gateway::{DROPPABLE_EVENTS, EventTypeFlags};
use twilight_model::gateway::Intents;
use twilight_model::gateway::event::EventType;

use crate::BuildViolation;
use crate::component::{ComponentHandler, ModalHandler};
use crate::events::EventHandler;
use crate::module::Module;

use self::commands::{Commands, Folded};

/// The intents Discord gates behind the Developer Portal.
const PRIVILEGED: Intents = Intents::GUILD_MEMBERS
    .union(Intents::GUILD_PRESENCES)
    .union(Intents::MESSAGE_CONTENT);

/// One folded registration.
pub(crate) struct Entry {
    /// The id of the Module it was added to.
    pub(crate) module: &'static str,
    /// The ids of that Module's ancestors, root first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Middleware (Story 7.6) walks it; tests read it now"
        )
    )]
    pub(crate) ancestors: Arc<[&'static str]>,
    pub(crate) handler: EventHandler,
}

/// The handler of one Route id: a component or a modal.
pub(crate) enum RouteHandler {
    Component(ComponentHandler),
    Modal(ModalHandler),
}

impl RouteHandler {
    /// The handler's struct path.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Component(h) => h.name(),
            Self::Modal(h) => h.name(),
        }
    }

    /// The Route id.
    fn route(&self) -> &'static str {
        match self {
            Self::Component(h) => h.route(),
            Self::Modal(h) => h.route(),
        }
    }

    /// Whether the Route is `signed`.
    fn signed(&self) -> bool {
        match self {
            Self::Component(h) => h.signed(),
            Self::Modal(h) => h.signed(),
        }
    }
}

/// One folded Route registration.
pub(crate) struct RouteEntry {
    /// The id of the Module it was added to.
    pub(crate) module: &'static str,
    /// The ids of that Module's ancestors, root first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Middleware (Story 7.6) walks it; tests read it now"
        )
    )]
    pub(crate) ancestors: Arc<[&'static str]>,
    pub(crate) handler: RouteHandler,
}

/// The handlers dispatched on one event type, in fold order.
#[derive(Default)]
pub(crate) struct Handlers {
    pub(crate) ordered: Vec<Arc<Entry>>,
    pub(crate) concurrent: Vec<Arc<Entry>>,
}

impl Handlers {
    fn all_droppable(&self) -> bool {
        self.ordered
            .iter()
            .chain(&self.concurrent)
            .all(|e| e.handler.droppable())
    }
}

/// Every registration of an App, folded once by `build()`.
pub(crate) struct Registry {
    entries: Vec<Arc<Entry>>,
    handlers: HashMap<EventType, Arc<Handlers>>,
    event_types: EventTypeFlags,
    droppable: EventTypeFlags,
    intents: Intents,
    commands: Commands,
    /// Every component and modal handler by Route id (AD-25): one
    /// immutable map, looked up with a borrowed `&str`.
    routes: HashMap<&'static str, Arc<RouteEntry>>,
    /// The ids of the mounted `signed` Routes, in fold order (CID-06).
    signed: Vec<&'static str>,
}

impl Registry {
    /// Folds `roots` pre-order: a Module's own registrations in `add`
    /// order, then each child. Returns the Registry and its violations: one
    /// [`BuildViolation::MissingIntent`] per handler whose privileged intent
    /// is not in `requested`, in fold order, then one
    /// [`BuildViolation::Command`] per command rule broken after merging
    /// every Module's commands (`validate_merge` order), then one
    /// [`BuildViolation::DuplicateRoute`] per extra handler of a Route id
    /// and one [`BuildViolation::UnmountedRoute`] per (Module, Route) a
    /// Module `uses` without a mounted handler, each in fold order.
    pub(crate) fn fold(roots: &[Module], requested: Intents) -> (Self, Vec<BuildViolation>) {
        let mut entries = Vec::new();
        let mut folded = Vec::new();
        let mut route_entries = Vec::new();
        let mut stack: Vec<(&Module, Arc<[&'static str]>)> = roots
            .iter()
            .rev()
            .map(|root| (root, Arc::from([])))
            .collect();
        while let Some((module, ancestors)) = stack.pop() {
            folded.push(Folded {
                module,
                ancestors: Arc::clone(&ancestors),
            });
            entries.extend(module.handlers().iter().map(|handler| {
                Arc::new(Entry {
                    module: module.id(),
                    ancestors: Arc::clone(&ancestors),
                    handler: handler.clone(),
                })
            }));
            let handlers = module
                .components()
                .iter()
                .cloned()
                .map(RouteHandler::Component)
                .chain(module.modals().iter().cloned().map(RouteHandler::Modal));
            route_entries.extend(handlers.map(|handler| RouteEntry {
                module: module.id(),
                ancestors: Arc::clone(&ancestors),
                handler,
            }));
            if !module.children().is_empty() {
                let path: Arc<[&'static str]> =
                    ancestors.iter().copied().chain([module.id()]).collect();
                stack.extend(
                    module
                        .children()
                        .iter()
                        .rev()
                        .map(|child| (child, Arc::clone(&path))),
                );
            }
        }

        // Keyed by dispatch type, never by `KIND`: an `UnavailableGuild`
        // handler shares the `GUILD_DELETE` slot with `GuildDelete` handlers.
        let mut by_dispatch: HashMap<EventType, Handlers> = HashMap::new();
        let mut event_types = EventTypeFlags::INTERACTION_CREATE;
        let mut intents = Intents::empty();
        let mut violations = Vec::new();
        for entry in &entries {
            let handler = &entry.handler;
            event_types |= handler.parse();
            let intent = handler.intent();
            if intent.intersects(PRIVILEGED) {
                if !requested.contains(intent) {
                    violations.push(BuildViolation::MissingIntent {
                        module: entry.module,
                        handler: handler.name(),
                        event: handler.kind().name().unwrap_or("UNKNOWN"),
                        intent,
                    });
                }
            } else {
                intents |= intent;
            }
            let slot = by_dispatch.entry(handler.dispatch()).or_default();
            if handler.ordered() {
                slot.ordered.push(Arc::clone(entry));
            } else {
                slot.concurrent.push(Arc::clone(entry));
            }
        }
        let droppable = by_dispatch
            .iter()
            .filter(|(_, handlers)| handlers.all_droppable())
            .fold(DROPPABLE_EVENTS, |flags, (dispatch, _)| {
                flags | EventTypeFlags::from(*dispatch)
            });
        let (commands, command_violations) = Commands::fold(&folded);
        violations.extend(command_violations);
        let (routes, signed) = fold_routes(route_entries, &mut violations);
        check_uses(&folded, &routes, &mut violations);
        let registry = Self {
            commands,
            routes,
            signed,
            entries,
            handlers: by_dispatch
                .into_iter()
                .map(|(dispatch, handlers)| (dispatch, Arc::new(handlers)))
                .collect(),
            event_types,
            droppable,
            intents,
        };
        (registry, violations)
    }

    /// The merged command Registry (empty when its checks failed).
    pub(crate) fn commands(&self) -> &Commands {
        &self.commands
    }

    /// The handlers dispatched on `kind` (an event's `Event::kind()`), if
    /// any.
    pub(crate) fn handlers(&self, kind: EventType) -> Option<&Arc<Handlers>> {
        self.handlers.get(&kind)
    }

    /// Every registration, in fold order.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[Arc<Entry>] {
        &self.entries
    }

    /// The handler of Route `id`, if any: one hash lookup with the
    /// borrowed key, no allocation.
    pub(crate) fn route(&self, id: &str) -> Option<&Arc<RouteEntry>> {
        self.routes.get(id)
    }

    /// The ids of the mounted `signed` Routes, in fold order.
    pub(crate) fn signed_routes(&self) -> &[&'static str] {
        &self.signed
    }

    /// How many registrations it holds: event handlers and Routes.
    pub(crate) fn len(&self) -> usize {
        self.entries.len() + self.routes.len()
    }

    /// The event types the gateway parses: interactions plus every handled
    /// type.
    pub(crate) fn event_types(&self) -> EventTypeFlags {
        self.event_types
    }

    /// The event types the event lane may drop.
    pub(crate) fn droppable(&self) -> EventTypeFlags {
        self.droppable
    }

    /// The non-privileged intents the handlers derive.
    pub(crate) fn intents(&self) -> Intents {
        self.intents
    }

    /// Whether any handler is ordered (the sub-queues exist only then).
    pub(crate) fn has_ordered(&self) -> bool {
        self.handlers.values().any(|h| !h.ordered.is_empty())
    }
}

/// Keeps the first handler of each Route id in fold order; each later one
/// is a [`BuildViolation::DuplicateRoute`]. Also returns the kept `signed`
/// Route ids, in fold order.
fn fold_routes(
    entries: Vec<RouteEntry>,
    violations: &mut Vec<BuildViolation>,
) -> (HashMap<&'static str, Arc<RouteEntry>>, Vec<&'static str>) {
    let mut routes: HashMap<&'static str, Arc<RouteEntry>> = HashMap::with_capacity(entries.len());
    let mut signed = Vec::new();
    for entry in entries {
        let route = entry.handler.route();
        if let Some(first) = routes.get(route) {
            violations.push(BuildViolation::DuplicateRoute {
                route,
                first_module: first.module,
                first_handler: first.handler.name(),
                second_module: entry.module,
                second_handler: entry.handler.name(),
            });
        } else {
            if entry.handler.signed() {
                signed.push(route);
            }
            routes.insert(route, Arc::new(entry));
        }
    }
    (routes, signed)
}

/// One [`BuildViolation::UnmountedRoute`] per (Module id, Route) that a
/// mounted Module `uses` and no mounted Module handles, in fold order.
fn check_uses(
    folded: &[Folded<'_>],
    routes: &HashMap<&'static str, Arc<RouteEntry>>,
    violations: &mut Vec<BuildViolation>,
) {
    let mut seen = HashSet::new();
    for module in folded.iter().map(|f| f.module) {
        for used in module.used_routes() {
            if !routes.contains_key(used.route) && seen.insert((module.id(), used.route)) {
                violations.push(BuildViolation::UnmountedRoute {
                    module: module.id(),
                    route: used.route,
                    route_type: used.route_type,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
