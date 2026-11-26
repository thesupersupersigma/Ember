use crate::{Plugin, PluginCtx, RequestAction, RequestFilter};

pub(crate) struct ClearUrls;

impl Plugin for ClearUrls {
    fn name(&self) -> &str {
        "clear-urls"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn author(&self) -> &str {
        "thesupersupersigma"
    }
    fn register(&self, ctx: &mut PluginCtx) {
        // TODO: body access needed — RequestInfo has method+path only, so the
        // handler cannot read or rewrite message content yet. Returns Pass
        // until RequestInfo gains a body field.
        ctx.on_request(
            RequestFilter {
                method: Some("POST"),
                path_prefix: Some("/channels/"),
                path_suffix: Some("/messages"),
            },
            |_req| RequestAction::Pass,
        );
        // TODO: also hook PATCH /channels/{id}/messages/{id} for edits once
        // RequestFilter supports path-contains or body access lands.
    }
}

// =========================================================================
// URL cleaning — pure transform, no I/O
// =========================================================================

/// Strip tracking query parameters from all URLs in `text`.
pub(crate) fn clean_urls(text: &str) -> String {
    if !text.contains('?') {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < bytes.len() {
        if let Some(scheme_len) = url_scheme_at(bytes, pos) {
            let start = pos;
            pos += scheme_len;
            // Consume URL characters. Balance parens for Wikipedia-style URLs.
            let mut paren_depth: u32 = 0;
            while pos < bytes.len() {
                match bytes[pos] {
                    b'(' => {
                        paren_depth += 1;
                        pos += 1;
                    }
                    b')' if paren_depth > 0 => {
                        paren_depth -= 1;
                        pos += 1;
                    }
                    b if b.is_ascii_whitespace() => break,
                    b'<' | b'>' | b'`' | b'|' | b')' => break,
                    _ => pos += 1,
                }
            }
            let raw = &text[start..pos];
            let url_end = trim_trailing_punctuation(raw);
            let url = &raw[..url_end];
            let suffix = &raw[url_end..];
            result.push_str(&clean_single_url(url));
            result.push_str(suffix);
        } else {
            let ch = text[pos..].chars().next().unwrap();
            result.push(ch);
            pos += ch.len_utf8();
        }
    }
    result
}

/// Returns the scheme byte length if `bytes[pos..]` starts with `http(s)://`.
fn url_scheme_at(bytes: &[u8], pos: usize) -> Option<usize> {
    let rest = &bytes[pos..];
    if rest.len() >= 8 && rest[..8].eq_ignore_ascii_case(b"https://") {
        Some(8)
    } else if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"http://") {
        Some(7)
    } else {
        None
    }
}

/// Byte length of the URL before trailing sentence punctuation
/// (`.` `,` `!` `?` `;` `:` `]` and markdown `**` `~~`).
fn trim_trailing_punctuation(url: &str) -> usize {
    let bytes = url.as_bytes();
    let mut end = bytes.len();
    while end > 0 {
        if end >= 2 && (bytes[end - 2..end] == *b"**" || bytes[end - 2..end] == *b"~~") {
            end -= 2;
        } else if matches!(
            bytes[end - 1],
            b'.' | b',' | b'!' | b'?' | b';' | b':' | b']'
        ) {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

/// Remove tracking query parameters from a single URL.
fn clean_single_url(url: &str) -> String {
    let query = match url.find('?') {
        Some(i) => i,
        None => return url.to_string(),
    };
    let hash = url.find('#');
    if hash.is_some_and(|h| h < query) {
        return url.to_string();
    }
    let end = hash.unwrap_or(url.len());
    let host = extract_host(url);
    let pairs: Vec<&str> = url[query + 1..end].split('&').collect();
    let kept: Vec<&str> = pairs
        .iter()
        .filter(|pair| {
            if pair.is_empty() {
                return false;
            }
            let raw_key = pair.split('=').next().unwrap_or("");
            let key = percent_decode(raw_key);
            !is_tracker(&key, &host)
        })
        .copied()
        .collect();
    if kept.len() == pairs.len() {
        return url.to_string();
    }
    let mut result = url[..query].to_string();
    if !kept.is_empty() {
        result.push('?');
        result.push_str(&kept.join("&"));
    }
    result.push_str(&url[end..]);
    result
}

/// Lowercase hostname from a URL, without port, userinfo, or trailing dot.
fn extract_host(url: &str) -> String {
    let after_scheme = url.find("//").map_or(0, |i| i + 2);
    let authority = &url[after_scheme..];
    let authority_end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    let authority = &authority[..authority_end];
    let host_start = authority.rfind('@').map_or(0, |i| i + 1);
    let host = authority[host_start..].split(':').next().unwrap_or("");
    let mut h = host.to_ascii_lowercase();
    if h.ends_with('.') {
        h.pop();
    }
    h
}

/// Minimal percent-decoding for query parameter keys.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    if !bytes.contains(&b'%') {
        return input.to_string();
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// True if `key` is a known tracking parameter (universal or host-scoped).
fn is_tracker(key: &str, host: &str) -> bool {
    if key.starts_with("utm_") {
        return true;
    }
    match key {
        // Ad platforms, email marketing, analytics.
        "fbclid" | "gclid" | "dclid" | "gbraid" | "wbraid" | "msclkid" | "yclid" | "twclid"
        | "ttclid" | "igshid" | "igsh" | "mc_eid" | "mc_cid" | "ref_src" | "ref_url"
        | "_hsenc" | "_hsmi" | "mkt_tok" | "oly_anon_id" | "oly_enc_id" | "vero_id"
        | "wickedid" | "__s" => true,
        // Host-scoped — these are common param names elsewhere (e.g. t= is start
        // time on YouTube).
        "s" | "t" => host_matches(host, &["twitter.com", "x.com", "tiktok.com"]),
        "feature" | "pp" => host_matches(host, &["youtube.com", "youtu.be"]),
        "si" => host_matches(host, &[
            "youtube.com",
            "youtu.be",
            "spotify.com",
            "soundcloud.com",
        ]),
        _ => false,
    }
}

/// True if `host` equals or is a subdomain of one of `domains`.
fn host_matches(host: &str, domains: &[&str]) -> bool {
    domains.iter().any(|&d| {
        host == d
            || (host.len() > d.len()
                && host.ends_with(d)
                && host.as_bytes()[host.len() - d.len() - 1] == b'.')
    })
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RequestFilter;

    #[test]
    fn filter_matches_message_send() {
        let filter = RequestFilter {
            method: Some("POST"),
            path_prefix: Some("/channels/"),
            path_suffix: Some("/messages"),
        };
        assert!(filter.matches("POST", "/channels/123456/messages"));
        assert!(!filter.matches("GET", "/channels/123/messages"));
        assert!(!filter.matches("POST", "/channels/123/typing"));
    }

    // -- is_tracker --

    #[test]
    fn utm_params() {
        for key in ["utm_source", "utm_medium", "utm_campaign", "utm_content", "utm_term"] {
            assert!(is_tracker(key, "example.com"), "{key}");
        }
    }

    #[test]
    fn ad_platform_trackers() {
        for key in [
            "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid",
        ] {
            assert!(is_tracker(key, "example.com"), "{key}");
        }
    }

    #[test]
    fn host_scoped_si() {
        assert!(is_tracker("si", "youtube.com"));
        assert!(is_tracker("si", "www.youtube.com"));
        assert!(is_tracker("si", "spotify.com"));
        assert!(is_tracker("si", "open.spotify.com"));
        assert!(is_tracker("si", "soundcloud.com"));
        assert!(!is_tracker("si", "example.com"));
    }

    #[test]
    fn host_scoped_social() {
        assert!(is_tracker("s", "twitter.com"));
        assert!(is_tracker("t", "x.com"));
        assert!(is_tracker("t", "tiktok.com"));
        assert!(!is_tracker("s", "youtube.com"));
        assert!(!is_tracker("t", "youtube.com"));
    }

    #[test]
    fn host_scoped_youtube_only() {
        assert!(is_tracker("feature", "youtube.com"));
        assert!(is_tracker("pp", "youtu.be"));
        assert!(!is_tracker("feature", "example.com"));
    }

    #[test]
    fn non_tracker_params() {
        assert!(!is_tracker("v", "youtube.com"));
        assert!(!is_tracker("q", "google.com"));
        assert!(!is_tracker("t", "youtube.com"));
        assert!(!is_tracker("id", "example.com"));
    }

    // -- extract_host --

    #[test]
    fn host_extraction() {
        assert_eq!(extract_host("https://www.youtube.com/watch?v=1"), "www.youtube.com");
        assert_eq!(extract_host("https://EXAMPLE.COM/page"), "example.com");
        assert_eq!(extract_host("https://user:pass@host.com/p"), "host.com");
        assert_eq!(extract_host("https://host.com:8080/p"), "host.com");
        assert_eq!(extract_host("https://host.com./p"), "host.com");
    }

    // -- clean_single_url --

    #[test]
    fn strips_all_trackers() {
        assert_eq!(
            clean_single_url("https://example.com/page?utm_source=tw&utm_medium=social"),
            "https://example.com/page"
        );
    }

    #[test]
    fn keeps_non_tracker_params() {
        let url = "https://youtube.com/watch?v=abc123";
        assert_eq!(clean_single_url(url), url);
    }

    #[test]
    fn mixed_params() {
        assert_eq!(
            clean_single_url("https://youtube.com/watch?v=abc&si=tracker&t=42"),
            "https://youtube.com/watch?v=abc&t=42"
        );
    }

    #[test]
    fn preserves_fragment() {
        assert_eq!(
            clean_single_url("https://example.com/page?fbclid=abc#section"),
            "https://example.com/page#section"
        );
    }

    #[test]
    fn fragment_before_query() {
        let url = "https://example.com/page#frag?notquery=1";
        assert_eq!(clean_single_url(url), url);
    }

    #[test]
    fn no_query_string() {
        let url = "https://example.com/page";
        assert_eq!(clean_single_url(url), url);
    }

    #[test]
    fn percent_encoded_tracker_key() {
        // %66 = 'f', so %66bclid = fbclid
        assert_eq!(
            clean_single_url("https://example.com/?%66bclid=abc"),
            "https://example.com/"
        );
    }

    // -- clean_urls (full text) --

    #[test]
    fn url_in_message_text() {
        assert_eq!(
            clean_urls("check this https://example.com/page?utm_source=tw wow"),
            "check this https://example.com/page wow"
        );
    }

    #[test]
    fn multiple_urls() {
        assert_eq!(
            clean_urls("https://a.com?fbclid=1 and https://b.com?gclid=2"),
            "https://a.com and https://b.com"
        );
    }

    #[test]
    fn trailing_punctuation_preserved() {
        assert_eq!(
            clean_urls("link: https://example.com/page?utm_source=x."),
            "link: https://example.com/page."
        );
    }

    #[test]
    fn no_urls_unchanged() {
        let text = "just a normal message with no links";
        assert_eq!(clean_urls(text), text);
    }

    #[test]
    fn url_without_trackers_unchanged() {
        let text = "see https://example.com/page?q=hello";
        assert_eq!(clean_urls(text), text);
    }

    #[test]
    fn wikipedia_parens() {
        assert_eq!(
            clean_urls("https://en.wikipedia.org/wiki/Rust_(programming_language)?utm_source=x"),
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
    }

    #[test]
    fn spotify_si() {
        assert_eq!(
            clean_urls("listen https://open.spotify.com/track/abc?si=xyz123"),
            "listen https://open.spotify.com/track/abc"
        );
    }

    #[test]
    fn twitter_scoped_params() {
        assert_eq!(
            clean_urls("https://x.com/user/status/123?s=20&t=abc"),
            "https://x.com/user/status/123"
        );
    }

    #[test]
    fn markdown_bold_suffix() {
        assert_eq!(
            clean_urls("**https://example.com/?fbclid=x**"),
            "**https://example.com/**"
        );
    }

    #[test]
    fn empty_text() {
        assert_eq!(clean_urls(""), "");
    }

    #[test]
    fn question_mark_no_url() {
        let text = "what? nothing here?";
        assert_eq!(clean_urls(text), text);
    }
}
