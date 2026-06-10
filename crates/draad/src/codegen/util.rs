//! Small string helpers used by the parser and both emitters. Kept here
//! to avoid the emitters importing from each other (or from the parser)
//! just to get at a name-mangling utility.

/// `snake_case` → `camelCase`. Used to derive TS method/event names from
/// their Rust counterparts.
pub(super) fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper = false;
    for c in s.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Uppercases the first character, leaves the rest untouched.
pub(super) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Returns the final `::`-separated segment of a Rust path. Falls back to
/// the whole string when the type carries generics (`Vec<Hit>` etc.) or
/// looks like a `use`-group (`crate::rpc::{Response, ok}`) — those need
/// to round-trip unchanged because the caller is using them as a full
/// `use` clause, not a single type name.
pub(super) fn last_path_segment(rust_ty: &str) -> String {
    if rust_ty.contains('<') {
        return rust_ty.to_string();
    }
    let plain = rust_ty
        .split('{')
        .next()
        .unwrap_or(rust_ty)
        .trim_end_matches("::");
    plain.rsplit("::").next().unwrap_or(plain).to_string()
}

/// Replaces every `bigint` token in a TS line with `number`. ts-rs maps
/// i64/u64/usize/isize → `bigint`, but our method-arg mapping uses
/// `number` and most app-domain ids/byte counts fit safely in JS Number
/// range.
pub(super) fn normalize_numbers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        if bytes[i..].starts_with(b"bigint") {
            let prev_ok = i == 0 || !is_word_char(bytes[i - 1]);
            let after = i + 6;
            let next_ok = after >= bytes.len() || !is_word_char(bytes[after]);
            if prev_ok && next_ok {
                out.push_str("number");
                i = after;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
