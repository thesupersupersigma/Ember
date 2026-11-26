use crate::{Plugin, PluginCtx, RequestAction, RequestFilter};

pub(crate) struct SilentTyping;

impl Plugin for SilentTyping {
    fn name(&self) -> &str {
        "silent-typing"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn author(&self) -> &str {
        "ember"
    }
    fn register(&self, ctx: &mut PluginCtx) {
        ctx.on_request(
            RequestFilter {
                method: Some("POST"),
                path_prefix: Some("/channels/"),
                path_suffix: Some("/typing"),
            },
            |_req| RequestAction::Drop,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::RequestFilter;

    #[test]
    fn filter_matches_silent_typing() {
        let filter = RequestFilter {
            method: Some("POST"),
            path_prefix: Some("/channels/"),
            path_suffix: Some("/typing"),
        };
        assert!(filter.matches("POST", "/channels/123456789/typing"));
        assert!(filter.matches("post", "/channels/999/typing"));
        assert!(!filter.matches("GET", "/channels/123/typing"));
        assert!(!filter.matches("POST", "/channels/123/messages"));
        assert!(!filter.matches("POST", "/users/@me/typing"));
    }

    #[test]
    fn filter_strips_query_string() {
        let filter = RequestFilter {
            method: None,
            path_prefix: Some("/channels/"),
            path_suffix: Some("/typing"),
        };
        assert!(filter.matches("POST", "/channels/123/typing?nonce=abc"));
        assert!(filter.matches("POST", "/channels/123/typing#frag"));
    }
}
