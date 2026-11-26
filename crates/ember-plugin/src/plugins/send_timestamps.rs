use crate::{Plugin, PluginCtx, RequestAction, RequestFilter};

pub(crate) struct SendTimestamps;

impl Plugin for SendTimestamps {
    fn name(&self) -> &str {
        "send-timestamps"
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
                crate::transform_message_content(req.body, replace_timestamps)
                    .map_or(RequestAction::Pass, RequestAction::Replace)
            },
        );
    }
}

/// Parse backtick-interior like "12:00", "3:51PM", "12:00 AM" into 24-hour (hour, minute).
fn parse_time(s: &str) -> Option<(u32, u32)> {
    let b = s.as_bytes();
    let len = b.len();
    let mut i = 0;

    // 1-2 digit hour
    if i >= len || !b[i].is_ascii_digit() {
        return None;
    }
    let mut hour = (b[i] - b'0') as u32;
    i += 1;
    if i < len && b[i].is_ascii_digit() {
        hour = hour * 10 + (b[i] - b'0') as u32;
        i += 1;
    }

    if i >= len || b[i] != b':' {
        return None;
    }
    i += 1;

    // exactly 2-digit minute
    if i + 2 > len || !b[i].is_ascii_digit() || !b[i + 1].is_ascii_digit() {
        return None;
    }
    let minute = (b[i] - b'0') as u32 * 10 + (b[i + 1] - b'0') as u32;
    i += 2;
    if minute > 59 {
        return None;
    }

    // optional single space before AM/PM
    if i < len && b[i] == b' ' {
        i += 1;
    }

    // optional AM/PM suffix — must consume the rest of the string
    if i < len {
        let tail = &s[i..];
        if tail.eq_ignore_ascii_case("AM") {
            if !(1..=12).contains(&hour) {
                return None;
            }
            if hour == 12 {
                hour = 0;
            }
        } else if tail.eq_ignore_ascii_case("PM") {
            if !(1..=12).contains(&hour) {
                return None;
            }
            if hour != 12 {
                hour += 12;
            }
        } else {
            return None;
        }
    } else if hour > 23 {
        return None;
    }

    Some((hour, minute))
}

/// Unix timestamp for today at `hour:minute`; bumps to tomorrow if already past.
// shortcut: fixed utc_offset (no DST), upgrade when platform TZ API lands
fn today_at(hour: u32, minute: u32, now_secs: i64, utc_offset_secs: i32) -> i64 {
    let local_now = now_secs + utc_offset_secs as i64;
    let day_start = local_now - local_now.rem_euclid(86400);
    let target_local = day_start + hour as i64 * 3600 + minute as i64 * 60;
    let target_utc = target_local - utc_offset_secs as i64;
    if target_utc <= now_secs {
        target_utc + 86400
    } else {
        target_utc
    }
}

/// Replace backtick-wrapped time patterns with Discord `<t:UNIX:t>` timestamps.
fn replace_timestamps_with_ref(text: &str, now_secs: i64, utc_offset_secs: i32) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('`') {
            Some(close) => {
                let inner = &after[..close];
                if let Some((h, m)) = parse_time(inner) {
                    let ts = today_at(h, m, now_secs, utc_offset_secs);
                    out.push_str(&format!("<t:{}:t>", ts));
                } else {
                    out.push('`');
                    out.push_str(inner);
                    out.push('`');
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('`');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Replace backtick-wrapped time patterns using system clock (UTC until TZ API is wired).
pub(crate) fn replace_timestamps(text: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    replace_timestamps_with_ref(text, now, 0)
}

// Skipped: composer picker button/modal UI — later UI addition

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RequestFilter;

    // 2023-11-14 22:13:20 UTC  —  day_start = 1_699_920_000
    const NOW: i64 = 1_700_000_000;
    const UTC: i32 = 0;

    // --- parse_time ---

    #[test]
    fn parse_24h() {
        assert_eq!(parse_time("12:00"), Some((12, 0)));
        assert_eq!(parse_time("3:51"), Some((3, 51)));
        assert_eq!(parse_time("17:59"), Some((17, 59)));
        assert_eq!(parse_time("0:00"), Some((0, 0)));
        assert_eq!(parse_time("23:59"), Some((23, 59)));
    }

    #[test]
    fn parse_am_pm() {
        assert_eq!(parse_time("12:00 AM"), Some((0, 0)));
        assert_eq!(parse_time("12:00 PM"), Some((12, 0)));
        assert_eq!(parse_time("3:51PM"), Some((15, 51)));
        assert_eq!(parse_time("3:51 pm"), Some((15, 51)));
        assert_eq!(parse_time("1:00AM"), Some((1, 0)));
        assert_eq!(parse_time("1:00PM"), Some((13, 0)));
        assert_eq!(parse_time("11:59 Pm"), Some((23, 59)));
    }

    #[test]
    fn parse_invalid() {
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("25:00"), None);
        assert_eq!(parse_time("12:60"), None);
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("abc"), None);
        assert_eq!(parse_time("12"), None);
        assert_eq!(parse_time("12:"), None);
        assert_eq!(parse_time("12:0"), None);
        assert_eq!(parse_time(":00"), None);
        assert_eq!(parse_time("0:00AM"), None);
        assert_eq!(parse_time("13:00PM"), None);
        assert_eq!(parse_time("12:00 XM"), None);
    }

    // --- today_at ---

    #[test]
    fn future_time_stays_today() {
        assert_eq!(today_at(23, 0, NOW, UTC), 1_700_002_800);
    }

    #[test]
    fn past_time_bumps_tomorrow() {
        assert_eq!(today_at(12, 0, NOW, UTC), 1_700_049_600);
    }

    #[test]
    fn midnight_past_bumps() {
        assert_eq!(today_at(0, 0, NOW, UTC), 1_700_006_400);
    }

    #[test]
    fn timezone_offset() {
        // UTC-5: local 17:13, so 18:00 local = 23:00 UTC — still future
        assert_eq!(today_at(18, 0, NOW, -18_000), 1_700_002_800);
    }

    // --- replace_timestamps_with_ref ---

    #[test]
    fn replace_single() {
        assert_eq!(
            replace_timestamps_with_ref("meet at `23:00` ok?", NOW, UTC),
            "meet at <t:1700002800:t> ok?"
        );
    }

    #[test]
    fn replace_am_pm() {
        assert_eq!(
            replace_timestamps_with_ref("call at `3:51PM`", NOW, UTC),
            "call at <t:1700063460:t>"
        );
    }

    #[test]
    fn replace_multiple() {
        assert_eq!(
            replace_timestamps_with_ref("`12:00` to `23:00`", NOW, UTC),
            "<t:1700049600:t> to <t:1700002800:t>"
        );
    }

    #[test]
    fn replace_invalid_unchanged() {
        let text = "see `24:00` and `hello`";
        assert_eq!(replace_timestamps_with_ref(text, NOW, UTC), text);
    }

    #[test]
    fn replace_no_backticks() {
        assert_eq!(replace_timestamps_with_ref("plain text", NOW, UTC), "plain text");
    }

    #[test]
    fn replace_unmatched_backtick() {
        assert_eq!(replace_timestamps_with_ref("oops `12:00", NOW, UTC), "oops `12:00");
    }

    // --- request filter ---

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

    #[test]
    fn body_transform_replaces_time() {
        let body = br#"{"content":"meet at `23:00` ok?","nonce":"1"}"#;
        let result = crate::transform_message_content(body, |text| {
            replace_timestamps_with_ref(text, NOW, UTC)
        });
        let parsed: serde_json::Value =
            serde_json::from_slice(result.as_ref().unwrap()).unwrap();
        assert_eq!(parsed["content"], "meet at <t:1700002800:t> ok?");
        assert_eq!(parsed["nonce"], "1");
    }

    #[test]
    fn body_transform_no_times_passes() {
        let body = br#"{"content":"no times here","nonce":"1"}"#;
        assert!(crate::transform_message_content(body, replace_timestamps).is_none());
    }
}
