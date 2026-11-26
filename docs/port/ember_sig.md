# ember-sig

Finds code in a binary by its byte shape instead of by address. Phoenix ships
no symbols and functions move every build, so each patch site is described as a
masked byte pattern and searched for at runtime.

## Pattern strings

IDA-style: whitespace-separated tokens, each either two hex digits (a byte that
must match) or `?` / `??` (any byte). Hex is case-insensitive.

`"83 79 ?? 03"` becomes

| position | 0    | 1    | 2    | 3    |
|----------|------|------|------|------|
| byte     | 0x83 | 0x79 | 0x00 | 0x03 |
| must match | yes | yes | no (wildcard) | yes |

A window of the haystack matches when every "must match" position is equal.

## Public API

- `Pattern::parse(&str) -> Result<Pattern, PatternError>` (also `str::parse` via `FromStr`)
- `Pattern::find(&self, &[u8]) -> Option<usize>`: first match offset
- `Pattern::find_all(&self, &[u8]) -> Vec<usize>`: every match offset, overlapping included
- `PatternError`: `Empty`, `BadChar { token, ch }`, `WrongLength { token, len }`; implements `Display` and `std::error::Error`. `token` is the 0-based token index.

## Edge cases

- Empty or whitespace-only pattern: parse error `Empty`, so a `Pattern` always has at least one byte.
- Single hex digit (`8`) or wrong-length token (`830`): `WrongLength`.
- Any char other than hex or `?`, including `0x` prefixes, commas or `???`: `BadChar`.
- Pattern longer than the haystack, or empty haystack: no match, no panic.
- All-wildcard pattern: matches at every offset `0..=hay.len() - pattern.len()`.
- Overlapping matches are all reported by `find_all`.

## Performance

v1 tests every window byte by byte. A `// PERF:` note in `find` marks where to
add a skip on the first concrete byte (memchr / Boyer-Moore-Horspool) before
scanning a ~500 MB `.text`.
