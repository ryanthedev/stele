//! Low-level scanners shared by the block and inline phases.
//!
//! Everything here is a pure function over `&str`/byte slices: character
//! classification (backed by the generated Unicode tables), entity lookup
//! and decoding, link-label normalization, and the small grammars the spec
//! defines exactly (link destinations/titles, HTML tags, autolinks).

use super::entities::ENTITIES;
use super::tables::{CASEFOLD, PUNCT_SYMBOL, ZS};

/// Unicode whitespace per CommonMark: Zs, tab, LF, FF, CR.
pub fn is_unicode_whitespace(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\x0C' | '\r' | ' ') || in_ranges(ZS, ch as u32)
}

/// Unicode punctuation per CommonMark 0.31.2: general categories P and S.
pub fn is_unicode_punctuation(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_punctuation()
    } else {
        in_ranges(PUNCT_SYMBOL, ch as u32)
    }
}

fn in_ranges(ranges: &[(u32, u32)], cp: u32) -> bool {
    ranges
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                std::cmp::Ordering::Greater
            } else if cp > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Full case folding of one char (C+F), for link-label unification.
fn casefold_char(ch: char, out: &mut String) {
    match CASEFOLD.binary_search_by_key(&(ch as u32), |&(cp, _)| cp) {
        Ok(i) => out.push_str(CASEFOLD[i].1),
        Err(_) => out.push(ch),
    }
}

/// Normalize a link label: strip brackets' inner whitespace at the ends,
/// collapse internal whitespace runs to one space, case-fold.
pub fn normalize_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut in_ws = false;
    for ch in label.trim_matches(|c: char| c.is_whitespace()).chars() {
        if ch.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws {
                out.push(' ');
                in_ws = false;
            }
            casefold_char(ch, &mut out);
        }
    }
    out
}

/// Look up a named entity (name without `&`/`;`).
pub fn lookup_entity(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by_key(&name, |&(n, _)| n)
        .ok()
        .map(|i| ENTITIES[i].1)
}

/// Scan an entity reference at `s[pos..]` where `s[pos] == '&'`.
/// Returns (byte length including `&` and `;`, decoded string).
pub fn scan_entity(s: &str, pos: usize) -> Option<(usize, String)> {
    let rest = &s.as_bytes()[pos..];
    debug_assert_eq!(rest.first(), Some(&b'&'));
    if rest.len() < 3 {
        return None;
    }
    if rest[1] == b'#' {
        let (digits_start, radix, max_len): (usize, u32, usize) =
            if rest.len() > 2 && (rest[2] == b'x' || rest[2] == b'X') {
                (3, 16, 6)
            } else {
                (2, 10, 7)
            };
        let mut i = digits_start;
        while i < rest.len() && (rest[i] as char).is_digit(radix) {
            i += 1;
        }
        let ndigits = i - digits_start;
        if ndigits == 0 || ndigits > max_len || rest.get(i) != Some(&b';') {
            return None;
        }
        let val = u32::from_str_radix(&s[pos + digits_start..pos + i], radix).ok()?;
        let ch = match char::from_u32(val) {
            Some('\0') | None => '\u{FFFD}',
            Some(c) => c,
        };
        Some((i + 1, ch.to_string()))
    } else {
        // Longest entity name is 31 chars.
        let mut i = 1;
        while i < rest.len() && i <= 32 && (rest[i] as char).is_ascii_alphanumeric() {
            i += 1;
        }
        if i == 1 || rest.get(i) != Some(&b';') {
            return None;
        }
        let decoded = lookup_entity(&s[pos + 1..pos + i])?;
        Some((i + 1, decoded.to_owned()))
    }
}

/// Decode backslash escapes and entity references in `s` (used for link
/// destinations, titles, info strings, and reference labels' targets).
pub fn unescape_string(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() => {
                out.push(bytes[i + 1] as char);
                i += 2;
            }
            b'&' => {
                if let Some((len, decoded)) = scan_entity(s, i) {
                    out.push_str(&decoded);
                    i += len;
                } else {
                    out.push('&');
                    i += 1;
                }
            }
            _ => {
                let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// Scan a link destination at `s[pos..]`.
/// Returns (end offset, raw destination without angle brackets).
pub fn scan_link_destination(s: &str, pos: usize) -> Option<(usize, &str)> {
    let bytes = s.as_bytes();
    if bytes.get(pos) == Some(&b'<') {
        // Pointy form: no newlines, no unescaped `<` or `>` inside.
        let mut i = pos + 1;
        while i < bytes.len() {
            match bytes[i] {
                b'>' => return Some((i + 1, &s[pos + 1..i])),
                b'<' | b'\n' | b'\r' => return None,
                b'\\' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() => i += 2,
                _ => i += 1,
            }
        }
        None
    } else {
        // Bare form: nonempty, no ASCII control or space, balanced parens.
        let mut i = pos;
        let mut parens: i32 = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() => i += 2,
                b'(' => {
                    parens += 1;
                    if parens > 32 {
                        return None; // pathological-nesting guard (cmark does likewise)
                    }
                    i += 1;
                }
                b')' => {
                    if parens == 0 {
                        break;
                    }
                    parens -= 1;
                    i += 1;
                }
                c if c <= 0x20 || c == 0x7F => break,
                _ => i += 1,
            }
        }
        if i == pos || parens != 0 {
            return None;
        }
        Some((i, &s[pos..i]))
    }
}

/// Scan a link title at `s[pos..]` (must start with `"`, `'`, or `(`).
/// Returns (end offset, raw title without delimiters).
pub fn scan_link_title(s: &str, pos: usize) -> Option<(usize, &str)> {
    let bytes = s.as_bytes();
    let open = *bytes.get(pos)?;
    let close = match open {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut i = pos + 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == close {
            return Some((i + 1, &s[pos + 1..i]));
        }
        if c == b'(' && open == b'(' {
            return None; // unescaped ( not allowed inside (-titles
        }
        if c == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// Scan a link label at `s[pos..]` where `s[pos] == '['`. Per spec: up to
/// 999 chars between brackets, no unescaped brackets inside.
/// Returns (end offset just past `]`, raw label between brackets).
pub fn scan_link_label(s: &str, pos: usize) -> Option<(usize, &str)> {
    let bytes = s.as_bytes();
    if bytes.get(pos) != Some(&b'[') {
        return None;
    }
    let mut i = pos + 1;
    let mut chars = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b']' => {
                return Some((i + 1, &s[pos + 1..i]));
            }
            b'[' => return None,
            b'\\' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() => {
                i += 2;
                chars += 2;
            }
            _ => {
                // Count characters, not bytes: the spec's limit is 999 chars,
                // so continuation bytes of a multi-byte char must not count.
                if bytes[i] & 0xC0 != 0x80 {
                    chars += 1;
                }
                i += 1;
            }
        }
        if chars > 999 {
            return None;
        }
    }
    None
}

// --- HTML constructs (spec §6.5 grammar, recognition only) ---

fn is_tag_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn is_tag_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}

fn is_attr_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b':'
}

fn is_attr_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b':' | b'-')
}

fn skip_html_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Scan an open or closing tag at `s[pos..]` where `s[pos] == '<'`.
/// Returns the end offset just past `>`.
pub fn scan_html_tag(s: &str, pos: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = pos + 1;
    let closing = if bytes.get(i) == Some(&b'/') {
        i += 1;
        true
    } else {
        false
    };
    if !is_tag_name_start(*bytes.get(i)?) {
        return None;
    }
    i += 1;
    while i < bytes.len() && is_tag_name_char(bytes[i]) {
        i += 1;
    }
    if !closing {
        // Attributes: each preceded by whitespace.
        loop {
            let after_ws = skip_html_ws(bytes, i);
            if after_ws == i || after_ws >= bytes.len() {
                i = after_ws;
                break;
            }
            if !is_attr_name_start(bytes[after_ws]) {
                i = after_ws;
                break;
            }
            i = after_ws + 1;
            while i < bytes.len() && is_attr_name_char(bytes[i]) {
                i += 1;
            }
            // Optional value.
            let eq = skip_html_ws(bytes, i);
            if bytes.get(eq) == Some(&b'=') {
                let v = skip_html_ws(bytes, eq + 1);
                match bytes.get(v)? {
                    b'"' => {
                        let end = bytes[v + 1..].iter().position(|&c| c == b'"')?;
                        i = v + 1 + end + 1;
                    }
                    b'\'' => {
                        let end = bytes[v + 1..].iter().position(|&c| c == b'\'')?;
                        i = v + 1 + end + 1;
                    }
                    _ => {
                        let mut j = v;
                        while j < bytes.len()
                            && !matches!(
                                bytes[j],
                                b' ' | b'\t'
                                    | b'\n'
                                    | b'\r'
                                    | b'"'
                                    | b'\''
                                    | b'='
                                    | b'<'
                                    | b'>'
                                    | b'`'
                            )
                        {
                            j += 1;
                        }
                        if j == v {
                            return None;
                        }
                        i = j;
                    }
                }
            }
        }
    } else {
        i = skip_html_ws(bytes, i);
    }
    if !closing && bytes.get(i) == Some(&b'/') {
        i += 1;
    }
    if bytes.get(i) == Some(&b'>') {
        Some(i + 1)
    } else {
        None
    }
}

/// Scan an HTML comment / PI / declaration / CDATA at `s[pos..]`
/// (`s[pos] == '<'`). Returns end offset just past the construct.
pub fn scan_html_special(s: &str, pos: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let rest = &bytes[pos..];
    if rest.starts_with(b"<!--") {
        // 0.31.2 comments: `<!-->`, `<!--->` are valid; otherwise text
        // until `-->` that does not contain `--`... relaxed: ends at first
        // `-->`; `<!-->` and `<!--->` special-cased.
        if rest.starts_with(b"<!-->") {
            return Some(pos + 5);
        }
        if rest.starts_with(b"<!--->") {
            return Some(pos + 6);
        }
        let mut i = 4;
        while i + 2 < rest.len() {
            if &rest[i..i + 3] == b"-->" {
                return Some(pos + i + 3);
            }
            i += 1;
        }
        None
    } else if rest.starts_with(b"<?") {
        let mut i = 2;
        while i + 1 < rest.len() {
            if &rest[i..i + 2] == b"?>" {
                return Some(pos + i + 2);
            }
            i += 1;
        }
        None
    } else if rest.starts_with(b"<![CDATA[") {
        let mut i = 9;
        while i + 2 < rest.len() {
            if &rest[i..i + 3] == b"]]>" {
                return Some(pos + i + 3);
            }
            i += 1;
        }
        None
    } else if rest.starts_with(b"<!") && rest.get(2).is_some_and(|c| c.is_ascii_alphabetic()) {
        let mut i = 3;
        while i < rest.len() {
            if rest[i] == b'>' {
                return Some(pos + i + 1);
            }
            i += 1;
        }
        None
    } else {
        None
    }
}

/// Scan a CommonMark autolink at `s[pos..]` (`s[pos] == '<'`).
/// Returns (end offset, dest-with-scheme, display text, is_email).
pub fn scan_autolink(s: &str, pos: usize) -> Option<(usize, String, String, bool)> {
    let rest = &s[pos + 1..];
    let close = rest.find('>')?;
    let inner = &rest[..close];
    if inner.bytes().any(|c| c <= 0x20 || c == b'<') {
        return None;
    }
    let end = pos + 1 + close + 1;
    // URI autolink: scheme 2..32 chars starting with a letter.
    if let Some(colon) = inner.find(':') {
        let scheme = &inner[..colon];
        if (2..=32).contains(&scheme.len())
            && scheme.as_bytes()[0].is_ascii_alphabetic()
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
        {
            return Some((end, inner.to_owned(), inner.to_owned(), false));
        }
    }
    // Email autolink (spec's HTML5-ish production).
    let at = inner.find('@')?;
    let (local, domain) = (&inner[..at], &inner[at + 1..]);
    if local.is_empty()
        || !local.bytes().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    b'.' | b'!'
                        | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'/'
                        | b'='
                        | b'?'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'{'
                        | b'|'
                        | b'}'
                        | b'~'
                        | b'-'
                )
        })
    {
        return None;
    }
    if domain.is_empty() {
        return None;
    }
    for seg in domain.split('.') {
        if seg.is_empty()
            || seg.len() > 63
            || !seg.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || seg.starts_with('-')
            || seg.ends_with('-')
        {
            return None;
        }
    }
    Some((end, format!("mailto:{inner}"), inner.to_owned(), true))
}

/// Is `line` (already stripped of container prefixes) a thematic break?
pub fn is_thematic_break(line: &str) -> bool {
    let mut ch = None;
    let mut count = 0;
    for c in line.chars() {
        match c {
            ' ' | '\t' => {}
            '-' | '_' | '*' => {
                if ch.is_none() {
                    ch = Some(c);
                }
                if ch != Some(c) {
                    return false;
                }
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}
