use std::sync::OnceLock;

use crate::{Plugin, PluginCtx, RequestAction, RequestFilter};

pub(crate) struct TextReplace;

/// A literal find→replace rule.
struct Rule {
    find: String,
    replace: String,
}

/// Parses rules in Sunbird format: one per line, `find=>replace`.
/// Splits on the *last* `=>` so `find` may itself contain `=>`.
/// Blank lines and lines without `=>` are silently skipped.
fn parse_rules(text: &str) -> Vec<Rule> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            // rfind so the find-part can contain `=>`
            let sep = line.rfind("=>")?;
            let find = &line[..sep];
            let replace = &line[sep + 2..];
            if find.is_empty() {
                return None;
            }
            Some(Rule {
                find: find.to_owned(),
                replace: replace.to_owned(),
            })
        })
        .collect()
}

/// Applies each rule in order via literal string replacement.
fn apply_rules(content: &str, rules: &[Rule]) -> String {
    rules
        .iter()
        .fold(content.to_owned(), |acc, r| acc.replace(&r.find, &r.replace))
}

/// Load rules once from `%APPDATA%\ember\text_replace.txt`.
fn loaded_rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let Ok(dir) = std::env::var("APPDATA") else {
            return Vec::new();
        };
        let path = format!("{dir}\\ember\\text_replace.txt");
        std::fs::read_to_string(path)
            .map(|text| parse_rules(&text))
            .unwrap_or_default()
    })
}

impl Plugin for TextReplace {
    fn name(&self) -> &str {
        "text-replace"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn author(&self) -> &str {
        "thesupersupersigma"
    }
    fn register(&self, ctx: &mut PluginCtx) {
        ctx.on_request(
            RequestFilter {
                method: Some("POST"),
                path_prefix: Some("/channels/"),
                path_suffix: Some("/messages"),
            },
            |req| {
                let rules = loaded_rules();
                if rules.is_empty() {
                    return RequestAction::Pass;
                }
                crate::transform_message_content(req.body, |text| {
                    apply_rules(text, rules)
                })
                .map_or(RequestAction::Pass, RequestAction::Replace)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RequestFilter;

    // ── parse_rules ──────────────────────────────────────────────

    #[test]
    fn parse_basic() {
        let rules = parse_rules("hello=>world");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].find, "hello");
        assert_eq!(rules[0].replace, "world");
    }

    #[test]
    fn parse_multiple_lines() {
        let rules = parse_rules("a=>b\nc=>d\ne=>f");
        assert_eq!(rules.len(), 3);
    }

    #[test]
    fn parse_skips_blank_lines() {
        let rules = parse_rules("\n  \nhello=>world\n\n");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn parse_skips_lines_without_separator() {
        let rules = parse_rules("no separator here\nhello=>world");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn parse_arrow_in_find() {
        // `=>` in the find portion: split on the *last* `=>`
        let rules = parse_rules("a=>b=>c");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].find, "a=>b");
        assert_eq!(rules[0].replace, "c");
    }

    #[test]
    fn parse_empty_replace() {
        let rules = parse_rules("remove=>");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].find, "remove");
        assert_eq!(rules[0].replace, "");
    }

    #[test]
    fn parse_empty_find_skipped() {
        let rules = parse_rules("=>orphan");
        assert_eq!(rules.len(), 0);
    }

    // ── apply_rules ──────────────────────────────────────────────

    #[test]
    fn apply_single_rule() {
        let rules = parse_rules("hello=>hi");
        assert_eq!(apply_rules("hello world", &rules), "hi world");
    }

    #[test]
    fn apply_multiple_rules() {
        let rules = parse_rules("a=>b\nc=>d");
        assert_eq!(apply_rules("a c", &rules), "b d");
    }

    #[test]
    fn apply_no_match() {
        let rules = parse_rules("xyz=>123");
        assert_eq!(apply_rules("hello world", &rules), "hello world");
    }

    #[test]
    fn apply_overlapping() {
        // First rule produces text the second rule matches.
        let rules = parse_rules("a=>ab\nb=>c");
        assert_eq!(apply_rules("a", &rules), "ac");
    }

    #[test]
    fn apply_empty_rules() {
        let rules: Vec<Rule> = Vec::new();
        assert_eq!(apply_rules("unchanged", &rules), "unchanged");
    }

    // ── request filter ───────────────────────────────────────────

    #[test]
    fn filter_matches_message_send() {
        let filter = RequestFilter {
            method: Some("POST"),
            path_prefix: Some("/channels/"),
            path_suffix: Some("/messages"),
        };
        assert!(filter.matches("POST", "/channels/123456789/messages"));
        assert!(!filter.matches("GET", "/channels/123/messages"));
        assert!(!filter.matches("POST", "/channels/123/typing"));
    }

    // ── body transform ──────────────────────────────────────────

    #[test]
    fn body_transform_applies_rules() {
        let rules = parse_rules("hello=>hi\nworld=>earth");
        let body = br#"{"content":"hello world","nonce":"1"}"#;
        let result = crate::transform_message_content(body, |text| {
            apply_rules(text, &rules)
        });
        let parsed: serde_json::Value =
            serde_json::from_slice(result.as_ref().unwrap()).unwrap();
        assert_eq!(parsed["content"], "hi earth");
        assert_eq!(parsed["nonce"], "1");
    }

    #[test]
    fn body_transform_no_match_passes() {
        let rules = parse_rules("xyz=>123");
        let body = br#"{"content":"hello world","nonce":"1"}"#;
        assert!(
            crate::transform_message_content(body, |text| apply_rules(text, &rules)).is_none()
        );
    }
}
