//! The immutable Registry `build()` folds once from the mounted Module trees
//! (AD-7, AD-9, GW-05).

use std::collections::HashMap;
use std::sync::Arc;

use noctis_gateway::{DROPPABLE_EVENTS, EventTypeFlags};
use twilight_model::gateway::Intents;
use twilight_model::gateway::event::EventType;

use crate::BuildViolation;
use crate::events::EventHandler;
use crate::module::Module;

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

/// The handlers of one event type, in fold order.
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
}

impl Registry {
    /// Folds `roots` pre-order: a Module's own registrations in `add`
    /// order, then each child. Returns the Registry and one
    /// [`BuildViolation::MissingIntent`] per handler whose privileged intent
    /// is not in `requested`, in fold order.
    pub(crate) fn fold(roots: &[Module], requested: Intents) -> (Self, Vec<BuildViolation>) {
        let mut entries = Vec::new();
        let mut stack: Vec<(&Module, Arc<[&'static str]>)> = roots
            .iter()
            .rev()
            .map(|root| (root, Arc::from([])))
            .collect();
        while let Some((module, ancestors)) = stack.pop() {
            entries.extend(module.handlers().iter().map(|handler| {
                Arc::new(Entry {
                    module: module.id(),
                    ancestors: Arc::clone(&ancestors),
                    handler: handler.clone(),
                })
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

        let mut by_kind: HashMap<EventType, Handlers> = HashMap::new();
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
            let slot = by_kind.entry(handler.kind()).or_default();
            if handler.ordered() {
                slot.ordered.push(Arc::clone(entry));
            } else {
                slot.concurrent.push(Arc::clone(entry));
            }
        }
        let droppable = by_kind
            .iter()
            .filter(|(_, handlers)| handlers.all_droppable())
            .fold(DROPPABLE_EVENTS, |flags, (kind, _)| {
                flags | EventTypeFlags::from(*kind)
            });
        let registry = Self {
            entries,
            handlers: by_kind
                .into_iter()
                .map(|(kind, handlers)| (kind, Arc::new(handlers)))
                .collect(),
            event_types,
            droppable,
            intents,
        };
        (registry, violations)
    }

    /// The handlers of `kind`, if any.
    pub(crate) fn handlers(&self, kind: EventType) -> Option<&Arc<Handlers>> {
        self.handlers.get(&kind)
    }

    /// Every registration, in fold order.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[Arc<Entry>] {
        &self.entries
    }

    /// How many registrations it holds.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
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

#[cfg(test)]
mod tests;
