use crate::{Plugin, PluginCtx, RequestFilter};

pub(crate) struct Unindent;

/// Strip common leading indentation from inner lines of a ``` code block.
fn unindent_block(block: &str) -> String {
    let lines: Vec<&str> = block.split('\n').collect();
    if lines.len() < 2 {
        return block.to_string(); // inline code block, leave alone
    }

    let first = lines[0]; // ```lang
    let last_idx = lines.len() - 1;
    let (inner, suffix) = if lines[last_idx].trim() == "```" {
        (&lines[1..last_idx], lines[last_idx])
    } else {
        (&lines[1..], "")
    };

    // Tabs → 4 spaces, then find minimum indentation of non-empty lines
    let expanded: Vec<String> = inner.iter().map(|l| l.replace('\t', "    ")).collect();
    let min_indent = expanded
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);

    if min_indent == 0 {
        // Still need to return with tabs expanded
        let mut out = String::from(first);
        for line in &expanded {
            out.push('\n');
            out.push_str(line);
        }
        if !suffix.is_empty() {
            out.push('\n');
            out.push_str(suffix);
        }
        return out;
    }

    let mut out = String::from(first);
    for line in &expanded {
        out.push('\n');
        if line.len() >= min_indent {
            out.push_str(&line[min_indent..]);
        } else {
            out.push_str(line); // empty or short whitespace-only line
        }
    }
    if !suffix.is_empty() {
        out.push('\n');
        out.push_str(suffix);
    }
    out
}

/// Transform a full message: unindent every ``` code block.
fn unindent_message(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("```") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 3..];
        match after_open.find("```") {
            Some(end) => {
                let block = &rest[start..start + 3 + end + 3];
                result.push_str(&unindent_block(block));
                rest = &rest[start + 3 + end + 3..];
            }
            None => {
                // Unclosed block — pass through unchanged
                result.push_str(rest);
                return result;
            }
        }
    }
    result.push_str(rest);
    result
}

fn unindent_handler(req: &crate::RequestInfo) -> crate::RequestAction {
    crate::transform_message_content(req.body, unindent_message)
        .map_or(crate::RequestAction::Pass, crate::RequestAction::Replace)
}

impl Plugin for Unindent {
    fn name(&self) -> &str {
        "unindent"
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
            unindent_handler,
        );
        ctx.on_request(
            RequestFilter {
                method: Some("PATCH"),
                path_prefix: Some("/channels/"),
                path_suffix: None, // shortcut: also matches non-message PATCHes, safe
                // because transform_message_content returns None for bodies
                // without a string "content" field
            },
            unindent_handler,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_unindent() {
        let input = "```rust\n    fn main() {\n        println!(\"hi\");\n    }\n```";
        let expected = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
        assert_eq!(unindent_message(input), expected);
    }

    #[test]
    fn tabs_to_spaces() {
        let input = "```\n\t\tfoo\n\t\t\tbar\n```";
        let expected = "```\nfoo\n    bar\n```";
        assert_eq!(unindent_message(input), expected);
    }

    #[test]
    fn mixed_indentation() {
        let input = "```\n\t  a\n\t    b\n```";
        // \t → 4 spaces: "      a" (6) and "        b" (8), min=6
        let expected = "```\na\n  b\n```";
        assert_eq!(unindent_message(input), expected);
    }

    #[test]
    fn inline_code_block_untouched() {
        let input = "```hello world```";
        assert_eq!(unindent_message(input), input);
    }

    #[test]
    fn multiple_code_blocks() {
        let input = "text\n```\n    a\n    b\n```\nmid\n```\n      c\n      d\n```\nend";
        let expected = "text\n```\na\nb\n```\nmid\n```\nc\nd\n```\nend";
        assert_eq!(unindent_message(input), expected);
    }

    #[test]
    fn language_tag_preserved() {
        let input = "```python\n    print(1)\n```";
        let expected = "```python\nprint(1)\n```";
        assert_eq!(unindent_message(input), expected);
    }

    #[test]
    fn empty_lines_within_block() {
        let input = "```\n    a\n\n    b\n```";
        let expected = "```\na\n\nb\n```";
        assert_eq!(unindent_message(input), expected);
    }

    #[test]
    fn no_indentation_no_change() {
        let input = "```\nfoo\nbar\n```";
        assert_eq!(unindent_message(input), input);
    }

    #[test]
    fn body_transform_unindents() {
        let body =
            br#"{"content":"```rust\n    fn main() {\n        println!(\"hi\");\n    }\n```","nonce":"1"}"#;
        let result = crate::transform_message_content(body, unindent_message);
        let parsed: serde_json::Value =
            serde_json::from_slice(result.as_ref().unwrap()).unwrap();
        assert_eq!(
            parsed["content"],
            "```rust\nfn main() {\n    println!(\"hi\");\n}\n```"
        );
    }

    #[test]
    fn body_transform_no_codeblocks_passes() {
        let body = br#"{"content":"just text","nonce":"1"}"#;
        assert!(crate::transform_message_content(body, unindent_message).is_none());
    }
}
