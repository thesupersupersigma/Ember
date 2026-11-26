use crate::{EventAction, EventFilter, EventInfo, Plugin, PluginCtx};

/// Minimal proof plugin: registers a gateway event hook on `MESSAGE_CREATE`.
/// Demonstrates the `on_event` API without altering any event (always passes).
pub(crate) struct EventObserver;

impl Plugin for EventObserver {
    fn name(&self) -> &str {
        "event-observer"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn author(&self) -> &str {
        "ember"
    }
    fn register(&self, ctx: &mut PluginCtx) {
        ctx.on_event(
            EventFilter {
                event_name: "MESSAGE_CREATE",
            },
            |_info: &EventInfo| EventAction::Pass,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::EventFilter;

    #[test]
    fn filter_matches_message_create() {
        let filter = EventFilter {
            event_name: "MESSAGE_CREATE",
        };
        assert!(filter.matches("MESSAGE_CREATE"));
        assert!(filter.matches("message_create"));
        assert!(!filter.matches("MESSAGE_DELETE"));
    }

    #[test]
    fn filter_rejects_partial_name() {
        let filter = EventFilter {
            event_name: "MESSAGE_CREATE",
        };
        assert!(!filter.matches("MESSAGE_CREAT"));
        assert!(!filter.matches("MESSAGE_CREATE_EXTRA"));
    }
}
