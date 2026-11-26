//! ember-sig — find functions inside a native binary by byte pattern.
//!
//! This is the heart of Ember: Discord.exe ships no symbols and its internal
//! functions move every build, so we locate them by code shape (a masked byte
//! pattern) rather than by fixed address. Keep this crate pure and
//! OS-independent so it can be unit-tested against byte fixtures with no Discord
//! present.
//!
//! ```
//! use ember_sig::Pattern;
//! let pat: Pattern = "83 79 ?? 03".parse().unwrap();
//! assert_eq!(pat.find(&[0x90, 0x83, 0x79, 0x0C, 0x03]), Some(1));
//! ```

use std::fmt;
use std::str::FromStr;

/// A masked byte pattern: concrete bytes plus wildcard positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<u8>,
    /// `true` = byte must match, `false` = wildcard (its `bytes` entry is 0 and ignored).
    mask: Vec<bool>,
}

/// Why a pattern string failed to parse. `token` is the 0-based index of the
/// whitespace-separated token at fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// The string had no tokens at all.
    Empty,
    /// A token contained a character that is neither a hex digit nor `?`.
    BadChar { token: usize, ch: char },
    /// A hex token was not exactly two digits (e.g. odd nibble `8`, or `830`).
    WrongLength { token: usize, len: usize },
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty pattern"),
            Self::BadChar { token, ch } => write!(f, "token {token}: invalid character {ch:?}"),
            Self::WrongLength { token, len } => {
                write!(f, "token {token}: expected 2 hex digits, got {len}")
            }
        }
    }
}

impl std::error::Error for PatternError {}

impl Pattern {
    /// Parse an IDA-style pattern: space-separated hex bytes, `?` or `??` for
    /// any byte. Hex is case-insensitive; extra whitespace is ignored.
    ///
    /// ```
    /// use ember_sig::{Pattern, PatternError};
    /// assert!(Pattern::parse("83 79 ?? 03").is_ok());
    /// assert!(Pattern::parse("  e8 ?  ff  ").is_ok());
    /// assert_eq!(Pattern::parse(""), Err(PatternError::Empty));
    /// ```
    pub fn parse(s: &str) -> Result<Self, PatternError> {
        let mut bytes = Vec::new();
        let mut mask = Vec::new();
        for (token, tok) in s.split_whitespace().enumerate() {
            if tok == "?" || tok == "??" {
                bytes.push(0);
                mask.push(false);
                continue;
            }
            if let Some(ch) = tok.chars().find(|c| !c.is_ascii_hexdigit()) {
                return Err(PatternError::BadChar { token, ch });
            }
            if tok.len() != 2 {
                return Err(PatternError::WrongLength { token, len: tok.len() });
            }
            // Two ASCII hex digits, so this cannot fail.
            bytes.push(u8::from_str_radix(tok, 16).unwrap());
            mask.push(true);
        }
        if bytes.is_empty() {
            return Err(PatternError::Empty);
        }
        Ok(Self { bytes, mask })
    }

    /// Offset of the first match in `hay`, or `None`.
    ///
    /// ```
    /// use ember_sig::Pattern;
    /// let pat = Pattern::parse("AA ?? CC").unwrap();
    /// assert_eq!(pat.find(&[0x00, 0xAA, 0xBB, 0xCC]), Some(1));
    /// assert_eq!(pat.find(&[0xAA, 0xBB]), None);
    /// ```
    pub fn find(&self, hay: &[u8]) -> Option<usize> {
        // PERF: for a ~500 MB .text, skip ahead with memchr / Boyer-Moore-Horspool
        // on the first non-wildcard byte instead of testing every window.
        hay.windows(self.bytes.len()).position(|w| self.matches(w))
    }

    /// Offsets of every match in `hay`, overlapping matches included.
    ///
    /// ```
    /// use ember_sig::Pattern;
    /// let pat = Pattern::parse("AA AA").unwrap();
    /// assert_eq!(pat.find_all(&[0xAA, 0xAA, 0xAA]), vec![0, 1]);
    /// ```
    pub fn find_all(&self, hay: &[u8]) -> Vec<usize> {
        hay.windows(self.bytes.len())
            .enumerate()
            .filter(|(_, w)| self.matches(w))
            .map(|(i, _)| i)
            .collect()
    }

    fn matches(&self, window: &[u8]) -> bool {
        self.bytes
            .iter()
            .zip(&self.mask)
            .zip(window)
            .all(|((b, &m), h)| !m || b == h)
    }
}

impl FromStr for Pattern {
    type Err = PatternError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Pattern {
        Pattern::parse(s).unwrap()
    }

    const HAY: [u8; 8] = [0x10, 0x83, 0x79, 0x0C, 0x03, 0x20, 0x83, 0x79];

    #[test]
    fn exact_match() {
        assert_eq!(p("83 79 0C 03").find(&HAY), Some(1));
    }

    #[test]
    fn no_match() {
        assert_eq!(p("83 79 0D").find(&HAY), None);
        assert!(p("83 79 0D").find_all(&HAY).is_empty());
    }

    #[test]
    fn match_at_zero_and_last_offset() {
        assert_eq!(p("10 83").find(&HAY), Some(0));
        assert_eq!(p("20 83 79").find(&HAY), Some(5)); // 5 = HAY.len() - 3
    }

    #[test]
    fn wildcards_middle_start_end() {
        assert_eq!(p("83 ?? 0C").find(&HAY), Some(1));
        assert_eq!(p("?? 79 0C").find(&HAY), Some(1));
        assert_eq!(p("0C 03 ?").find(&HAY), Some(3));
        // trailing wildcard must not run past the end
        assert_eq!(p("83 79 ??").find_all(&HAY), vec![1]);
    }

    #[test]
    fn all_wildcards_match_every_offset() {
        assert_eq!(p("?? ??").find_all(&[1, 2, 3]), vec![0, 1]);
        assert_eq!(p("?").find(&[7]), Some(0));
    }

    #[test]
    fn find_all_multiple_and_overlapping() {
        assert_eq!(p("83 79").find_all(&HAY), vec![1, 6]);
        assert_eq!(p("AA ?? AA").find_all(&[0xAA; 5]), vec![0, 1, 2]);
    }

    #[test]
    fn pattern_longer_than_haystack() {
        assert_eq!(p("83 79 0C").find(&[0x83, 0x79]), None);
        assert!(p("?? ?? ??").find_all(&[0x83, 0x79]).is_empty());
    }

    #[test]
    fn empty_haystack() {
        assert_eq!(p("83").find(&[]), None);
        assert!(p("??").find_all(&[]).is_empty());
    }

    #[test]
    fn parse_case_and_whitespace() {
        assert_eq!(p("\t0c  aB\n?? "), p("0C AB ?"));
        assert_eq!("0c".parse::<Pattern>(), Ok(p("0C")));
    }

    #[test]
    fn parse_errors() {
        use PatternError::*;
        assert_eq!(Pattern::parse(""), Err(Empty));
        assert_eq!(Pattern::parse("   "), Err(Empty));
        assert_eq!(Pattern::parse("8"), Err(WrongLength { token: 0, len: 1 }));
        assert_eq!(Pattern::parse("83 830"), Err(WrongLength { token: 1, len: 3 }));
        assert_eq!(Pattern::parse("8Z"), Err(BadChar { token: 0, ch: 'Z' }));
        assert_eq!(Pattern::parse("83 ??? 03"), Err(BadChar { token: 1, ch: '?' }));
        assert_eq!(Pattern::parse("83,79"), Err(BadChar { token: 0, ch: ',' }));
        assert_eq!(Pattern::parse("0x83"), Err(BadChar { token: 0, ch: 'x' }));
        assert_eq!(Pattern::parse("8").unwrap_err().to_string(), "token 0: expected 2 hex digits, got 1");
    }
}
