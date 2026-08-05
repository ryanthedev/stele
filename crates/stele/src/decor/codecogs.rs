//! CodeCogs image links back into the TeX they were made from.
//!
//! Before GitHub rendered `$…$`, the way to put an equation in a README was
//! to hand it to `latex.codecogs.com` and embed the resulting image. Those
//! documents are still everywhere, and in a terminal they are unreadable:
//! the maths *is* the content, and the content is a URL. This turns
//!
//! ```text
//! ![equals1](http://latex.codecogs.com/svg.latex?x%20%3A%3D%202kj)
//! ```
//!
//! back into `$x := 2kj$`, which stele already renders — as a real formula
//! with `crates/math`, or as TeX source under `--no-images`.
//!
//! **Say it out loud:** under `--no-images` a reader used to see the author's
//! alt text and will now see TeX source instead. For the corpus file this
//! pass was built against that is a clear win — the alt texts there are
//! `dotcross5`-style slugs that name nothing — but it is a real change, and
//! for a document whose alt text was written with care it is a loss. The
//! alt text is not recoverable afterwards, because the image node it hung on
//! is gone.
//!
//! ## Why the decoder is hand-written
//!
//! The corpus mixes two encodings in one URL — `a&space;&plus;&space;ib` sits
//! two lines from `%5Csum_%7Bi%3D1%7D`. `&space;` and `&plus;` are **not**
//! HTML entities; they are a CodeCogs invention. A real entity decoder does
//! not know them (it would leave them as literal text, or worse, decode
//! `&plus;` — which *is* a valid HTML5 named entity — while leaving
//! `&space;` alone, producing a half-decoded formula). Percent-decoding and
//! these two names, and nothing else, is the whole rule.
//!
//! A bare `+` is left as a literal plus rather than read as the
//! `application/x-www-form-urlencoded` space. CodeCogs spells a real plus
//! `&plus;`, so a bare one is ambiguous — but dropping an operator out of a
//! formula is a worse error than a stray space in TeX, where spacing is
//! mostly insignificant anyway.

use std::borrow::Cow;

use super::fences::{Scan, strip_eol};

/// Rewrites every CodeCogs image link in `source` to inline `$…$` maths.
///
/// Returns `source` untouched when there is nothing to rewrite, and — per
/// R10 — when the document has an unclosed fence, since nothing can be said
/// with confidence about what is code in a half-written file.
pub fn apply(source: &str) -> Cow<'_, str> {
    let scan = Scan::of(source);
    if !scan.balanced() {
        return Cow::Borrowed(source);
    }

    let mut out = String::new();
    let mut rewrote = false;
    for (index, raw) in source.split_inclusive('\n').enumerate() {
        if scan.is_protected(index) {
            out.push_str(raw);
            continue;
        }
        let line = strip_eol(raw);
        match rewrite_line(line) {
            Some(rewritten) => {
                rewrote = true;
                out.push_str(&rewritten);
                out.push_str(&raw[line.len()..]);
            }
            None => out.push_str(raw),
        }
    }

    if rewrote {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(source)
    }
}

/// One line with its CodeCogs images replaced, or `None` when it has none.
fn rewrite_line(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut copied = 0usize;
    let mut cursor = 0usize;
    let mut rewrote = false;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'!' || bytes[cursor + 1] != b'[' {
            cursor += 1;
            continue;
        }
        let Some((dest, end)) = image_at(line, cursor) else {
            cursor += 1;
            continue;
        };
        let Some(tex) = query(dest).and_then(decode_tex) else {
            // Not a CodeCogs link, or one whose query does not decode to
            // usable TeX: leave the image exactly as the author wrote it.
            cursor = end;
            continue;
        };
        // Two CodeCogs images side by side — file 05 packs four into one
        // sentence — would emit `$a$$b$`, whose middle two dollars merge into
        // one run and leave a single formula reading `a$$b`. The same happens
        // against a `$` the author wrote themselves. A space between keeps
        // them two formulae; see `super::spaced_to_not_merge`.
        let before = crate::decor::char_before(&out, line, copied, cursor);
        let after = line[end..].chars().next();
        let rendered = format!("${tex}$");
        out.push_str(&line[copied..cursor]);
        out.push_str(&crate::decor::spaced_to_not_merge(before, &rendered, after));
        copied = end;
        cursor = end;
        rewrote = true;
    }

    if !rewrote {
        return None;
    }
    out.push_str(&line[copied..]);
    Some(out)
}

/// The link destination of the image starting at `start` (which must be
/// `![`), and the byte offset just past its closing `)`.
///
/// Bracket and parenthesis depth are both tracked: alt text legitimately
/// contains balanced brackets, and while a CodeCogs URL spells its own
/// parentheses `%28`/`%29`, some other image on the same line may not.
fn image_at(line: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    let mut cursor = start + 2;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    // The loop leaves in exactly two states: it broke out of the `]` arm with
    // `depth == 0`, or it ran past the end of the line with `depth >= 1`
    // (nothing else decrements `depth`, and reaching zero always breaks). So
    // "did the label close?" and "are we sitting on a `]`?" are the same
    // question, asked once, with the invariant asserted rather than re-tested
    // — a second `if` on it would be a branch no input can take.
    if depth != 0 {
        return None;
    }
    debug_assert_eq!(
        bytes.get(cursor),
        Some(&b']'),
        "depth reaches zero only at a `]`"
    );
    if bytes.get(cursor + 1) != Some(&b'(') {
        return None;
    }
    let dest_start = cursor + 2;
    let mut cursor = dest_start;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((line[dest_start..cursor].trim(), cursor + 1));
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

/// The percent-encoded TeX in `dest`, if `dest` is a CodeCogs endpoint.
///
/// Every endpoint variant a real document uses is accepted, not just the one
/// the corpus happens to hold: either scheme, and any of the six
/// `{svg,png,gif}.{latex,image}` renderers. They differ only in what bytes
/// come back from the server, which is not something this cares about.
fn query(dest: &str) -> Option<&str> {
    // A destination with a space in it carries a title (`](url "title")`) or
    // angle-bracket delimiters. Neither is part of the URL, and folding one
    // into the TeX would put the author's caption inside the formula — so
    // the whole image is left alone instead.
    if dest.contains(char::is_whitespace) || dest.starts_with('<') {
        return None;
    }
    let rest = dest
        .strip_prefix("https://")
        .or_else(|| dest.strip_prefix("http://"))
        .unwrap_or(dest);
    let rest = rest.strip_prefix("latex.codecogs.com/")?;
    let (endpoint, query) = rest.split_once('?')?;
    let (format, kind) = endpoint.split_once('.')?;
    if !matches!(format, "svg" | "png" | "gif") || !matches!(kind, "latex" | "image") {
        return None;
    }
    Some(query)
}

/// The TeX a CodeCogs query string encodes, or `None` when it does not
/// decode to something that can be spliced into `$…$`.
fn decode_tex(query: &str) -> Option<String> {
    // Byte-wise throughout, never `&query[cursor..]`: a percent escape can
    // land the cursor in the middle of a multi-byte character, and slicing a
    // `str` there panics.
    let bytes = query.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let rest = &bytes[cursor..];
        if rest.starts_with(b"&space;") {
            decoded.push(b' ');
            cursor += "&space;".len();
        } else if rest.starts_with(b"&plus;") {
            decoded.push(b'+');
            cursor += "&plus;".len();
        } else if rest[0] == b'%'
            && let Some(byte) = hex_byte(rest.get(1), rest.get(2))
        {
            decoded.push(byte);
            cursor += 3;
        } else {
            decoded.push(rest[0]);
            cursor += 1;
        }
    }

    // A percent escape may split a multi-byte character, so the bytes are
    // reassembled first and validated once. A query that decodes to invalid
    // UTF-8 is not TeX anybody wrote — leave the image alone.
    let mut tex = String::from_utf8(decoded).ok()?.trim().to_string();
    loop {
        let stripped = strip_render_directive(&tex).trim().to_string();
        if stripped == tex {
            break;
        }
        tex = stripped;
    }

    // `$` inside would close the span early and hand the rest of the formula
    // to the paragraph as prose; a newline cannot appear in an inline span at
    // all. Both mean "this is not safely expressible here", so the image
    // stays as it is.
    if tex.is_empty() || tex.contains('$') || tex.contains('\n') {
        return None;
    }
    Some(tex)
}

/// `tex` with one leading CodeCogs render directive removed.
///
/// `\inline` and `\dpi{110}` are instructions to the image server about how
/// to draw the formula, not part of the formula. Left in place they would
/// reach `crates/math` as unknown macros.
fn strip_render_directive(tex: &str) -> &str {
    if let Some(rest) = tex.strip_prefix("\\inline") {
        return rest;
    }
    for directive in ["\\dpi", "\\bg", "\\fg"] {
        if let Some(rest) = tex.strip_prefix(directive)
            && let Some(rest) = rest.trim_start().strip_prefix('{')
            && let Some(end) = rest.find('}')
        {
            return &rest[end + 1..];
        }
    }
    tex
}

/// One byte from two hex digits, or `None` if either is missing or not hex.
///
/// `char::to_digit(16)` is the rejection: it accepts `0-9a-fA-F` and nothing
/// else, so `%ZZ` comes back `None` and the `%` is copied through as the
/// literal the author wrote rather than silently becoming some other byte.
fn hex_byte(high: Option<&u8>, low: Option<&u8>) -> Option<u8> {
    let digit = |byte: Option<&u8>| char::from(*byte?).to_digit(16);
    let high = digit(high)?;
    let low = digit(low)?;
    u8::try_from(high * 16 + low).ok()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    /// Where the real CommonMark parser says the first image on `line` ends,
    /// and what destination it read.
    ///
    /// This is the oracle [`image_at`] is measured against, and it is a good
    /// one precisely because it shares no code with it: `crates/ast` decides
    /// the same three questions — how far a `[…]` label runs when it holds
    /// brackets of its own, how far a `(…)` destination runs when it holds
    /// parentheses of its own, and what a backslash does to either — from an
    /// entirely separate implementation of the CommonMark rules. A scanner
    /// that ends an image one byte from where the parser ends it is a
    /// scanner that will one day replace the wrong bytes of somebody's
    /// document.
    fn parser_image(line: &str) -> Option<(String, usize)> {
        let doc = ast::Document::parse(line);
        let ast::BlockKind::Paragraph { children } = &doc.blocks().first()?.kind else {
            return None;
        };
        children.iter().find_map(|inline| match &inline.kind {
            ast::InlineKind::Image { dest, .. } => Some((dest.clone(), inline.span.end)),
            _ => None,
        })
    }

    /// `dest` with CommonMark backslash escapes resolved.
    ///
    /// The parser reports a *decoded* destination and [`image_at`] reports
    /// the raw bytes (it hands them to [`query`], which needs the URL as
    /// written). Comparing the two therefore needs one of them normalised,
    /// and unescaping the raw form is the direction that cannot invent a
    /// character the source did not contain.
    fn unescape(dest: &str) -> String {
        let mut out = String::new();
        let mut chars = dest.chars().peekable();
        while let Some(ch) = chars.next() {
            match (ch, chars.peek()) {
                ('\\', Some(next)) if next.is_ascii_punctuation() => {
                    out.push(*next);
                    chars.next();
                }
                _ => out.push(ch),
            }
        }
        out
    }

    /// The scanner's whole job is to say which bytes of a line are an image.
    /// Every shape below is one the byte walk has to reason about — nested
    /// brackets, nested parentheses, and a backslash in front of either — and
    /// the parser is asked the same question independently.
    #[test]
    fn test_image_at_ends_an_image_exactly_where_the_commonmark_parser_ends_it() {
        for line in [
            "![a](url)",
            "![](url)",
            // Balanced brackets inside the alt text: the first `]` is not the
            // end of the label.
            "![a [b] c](url)",
            "![[bracketed]](url)",
            // A backslash-escaped bracket is not the end of the label either.
            "![a \\] b](url)",
            // Balanced and escaped parentheses inside the destination.
            "![a](http://e.com/f(x).png)",
            "![a](/img/f\\).png)",
            // An image that is not the first thing on the line, and one that
            // ends on the line's very last byte.
            "x ![a](u) y",
            "text then ![a](u)",
        ] {
            let (expected_dest, expected_end) = parser_image(line).unwrap_or_else(|| {
                panic!("{line:?} must parse as an image for this case to mean anything")
            });
            let start = line.find("![").expect("every case here holds an image");
            let (dest, end) = image_at(line, start)
                .unwrap_or_else(|| panic!("image_at found no image in {line:?}"));

            assert_eq!(end, expected_end, "end offset for {line:?}");
            assert_eq!(unescape(dest), expected_dest, "destination for {line:?}");
        }
    }

    /// The other half, and the more dangerous one: the scanner must not
    /// *invent* an image. Each of these has an `![` in it and none of them is
    /// an image, so a scanner that goes hunting for a closing `)` somewhere
    /// later on the line will find one and rewrite bytes that were never a
    /// link.
    #[test]
    fn test_image_at_refuses_every_shape_the_parser_also_refuses() {
        for line in [
            // A `]` that is not followed by `(` ends the matter — even though
            // there is a tempting `)` further along the line.
            "![alt]xy)",
            "![a] (url)",
            "![a]",
            // No closing bracket at all, and no closing parenthesis at all.
            "![unclosed alt",
            "![a](unclosed",
            "![a](f(x)",
            // Truncated to the last byte, in both places it can be truncated.
            "![a",
            "![",
        ] {
            assert_eq!(
                parser_image(line),
                None,
                "{line:?} must not parse as an image for this case to mean anything"
            );
            let start = line.find("![").expect("every case here holds an `![`");
            assert_eq!(image_at(line, start), None, "image_at accepted {line:?}");
        }
    }

    /// [`rewrite_line`] walks the line a byte at a time looking for `![`, and
    /// two ways that walk can go wrong are invisible to any test that only
    /// ever feeds it well-formed images: reading one byte past the last one,
    /// and failing to advance past an `![` that turns out to open nothing.
    /// The second is not a wrong answer but a hang — the scanner sits on the
    /// same byte forever, and the viewer never finishes loading the file.
    #[test]
    fn test_the_line_scanner_advances_and_stops_on_hostile_punctuation() {
        for src in [
            // A `!` as the very last byte of the line: there is no `[` after
            // it to look at.
            "That is amazing!\n",
            // An `![` that opens nothing, at the start of the line and away
            // from it. Both must leave the scanner somewhere it can make
            // progress from.
            "![unclosed alt text\n",
            "see ![unclosed and then more prose\n",
            "![a] and ![b] and ![c]\n",
            "![\n",
            "!\n",
            "\n",
            "",
        ] {
            assert!(
                matches!(apply(src), Cow::Borrowed(_)),
                "nothing here is a CodeCogs image: {src:?}"
            );
        }
    }

    /// Document-level, so the consequence is visible rather than inferred:
    /// alt text is arbitrary author prose and may hold brackets of its own.
    /// A scanner that ends the label at the first `]` finds no `(` after it,
    /// declines the image, and the reader keeps a URL where the equation
    /// should be.
    #[test]
    fn test_brackets_and_escapes_inside_alt_text_do_not_end_the_image_early() {
        for alt in ["a [b] c", "[bracketed]", "a \\] b", "[[deep]]"] {
            let src = format!("![{alt}](http://latex.codecogs.com/svg.latex?x%5E2)\n");
            assert_eq!(&*apply(&src), "$x^2$\n", "for alt text {alt:?}");
        }
    }

    /// The same at the other end of the image. Here the failure is worse than
    /// declining: ending the destination at the first `)` yields a *shorter*
    /// URL that still decodes, so the equation comes out truncated and the
    /// leftover bytes are left lying in the paragraph.
    #[test]
    fn test_parentheses_and_escapes_inside_a_destination_do_not_end_it_early() {
        let src = "![m](http://latex.codecogs.com/svg.latex?f(x))\n";
        assert_eq!(&*apply(src), "$f(x)$\n");

        let src = "![m](http://latex.codecogs.com/svg.latex?a\\)b)\n";
        assert_eq!(&*apply(src), "$a\\)b$\n");
    }

    /// Where the scanner resumes after an image is as load-bearing as where
    /// it starts: it resumes at `end`, so an `end` that is short re-scans
    /// bytes it already consumed and an `end` that is long steps over the
    /// next image entirely. Two images on one line, the first with brackets
    /// and parentheses in it, is what makes both mistakes visible.
    #[test]
    fn test_a_second_image_on_the_line_is_still_found_after_a_bracket_heavy_first() {
        let src = "![a [b] c](./img/f(1).png) then \
![m](http://latex.codecogs.com/svg.latex?y%5E2) done\n";
        assert_eq!(&*apply(src), "![a [b] c](./img/f(1).png) then $y^2$ done\n");
    }

    /// The oracle: the TeX these URLs encode is written out in the corpus
    /// itself, in the HTML comment the author put under each image
    /// (`<!-- \sum_{i=1}^{100}(2i+1) -->`). These expectations are copied from
    /// there rather than from what the decoder happens to produce.
    #[test]
    fn test_percent_encoded_and_pseudo_entity_urls_both_decode_to_their_tex() {
        let cases = [
            ("x%20%3A%3D%202kj", "x := 2kj"),
            ("a&space;&plus;&space;ib", "a + ib"),
            (
                "%5Csum_%7Bi%3D1%7D%5E%7B100%7D%282i&plus;1%29",
                "\\sum_{i=1}^{100}(2i+1)",
            ),
            ("%280%2C%201%29", "(0, 1)"),
            ("%5B0%2C%201%5D", "[0, 1]"),
            (
                "%5Cleft%28%5Csqrt%7Bx%7D%5Cright%29%5E2%20%3D%20x",
                "\\left(\\sqrt{x}\\right)^2 = x",
            ),
        ];
        for (encoded, expected) in cases {
            assert_eq!(
                decode_tex(encoded).as_deref(),
                Some(expected),
                "decoding {encoded}"
            );
        }
    }

    /// `&plus;` is a real HTML5 named entity and `&space;` is not, so an
    /// off-the-shelf entity decoder gets exactly one of these right. Both
    /// must survive, in the same string, in either order.
    #[test]
    fn test_the_two_codecogs_pseudo_entities_decode_and_nothing_else_does() {
        assert_eq!(decode_tex("&plus;&space;&plus;").as_deref(), Some("+ +"));
        // Genuine HTML entities are *not* decoded — `&amp;` in a formula is
        // a literal `\&`-less ampersand run, and guessing at it would be an
        // invention.
        assert_eq!(decode_tex("a&amp;b").as_deref(), Some("a&amp;b"));
    }

    #[test]
    fn test_every_endpoint_variant_a_real_document_uses_is_recognised() {
        for host in [
            "http://latex.codecogs.com/svg.latex?a",
            "https://latex.codecogs.com/svg.latex?a",
            "http://latex.codecogs.com/png.latex?a",
            "https://latex.codecogs.com/gif.latex?a",
            "https://latex.codecogs.com/svg.image?a",
            "latex.codecogs.com/png.image?a",
        ] {
            assert_eq!(query(host), Some("a"), "{host}");
        }
        // Neither a CodeCogs URL nor an endpoint it serves.
        assert_eq!(query("https://example.com/svg.latex?a"), None);
        assert_eq!(query("https://latex.codecogs.com/svg.latex"), None);
        assert_eq!(query("https://latex.codecogs.com/webp.latex?a"), None);
        // A title after the URL is the author's caption, not TeX.
        assert_eq!(
            query("https://latex.codecogs.com/svg.latex?a \"caption\""),
            None
        );
        assert!(matches!(
            apply("![m](http://latex.codecogs.com/svg.latex?x \"a caption\")\n"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_a_leading_render_directive_is_not_part_of_the_formula() {
        assert_eq!(decode_tex("%5Cinline%20x%5E2").as_deref(), Some("x^2"));
        assert_eq!(decode_tex("%5Cdpi%7B110%7Dx%5E2").as_deref(), Some("x^2"));
        assert_eq!(
            decode_tex("%5Cdpi%7B110%7D%5Cinline%20x%5E2").as_deref(),
            Some("x^2")
        );
        // A `\dpi` with no brace group is not a directive; leave it be.
        assert_eq!(decode_tex("%5Cdpi%20x").as_deref(), Some("\\dpi x"));
    }

    #[test]
    fn test_an_image_that_is_not_codecogs_is_left_exactly_as_written() {
        let src = "![a photo](./img/cat.png) and [a link](http://latex.codecogs.com/svg.latex?x)\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    /// The line in file 05 that packs four images into one sentence, with
    /// two different encodings among them. Asserted through the real parser:
    /// four `Math` inlines, in order, with the right TeX.
    #[test]
    fn test_four_images_in_one_sentence_all_become_maths_in_order() {
        let src = "Complex numbers are of the form \
![complex](http://latex.codecogs.com/svg.latex?a&space;&plus;&space;ib), where \
![a](http://latex.codecogs.com/svg.latex?a) is the real part and \
![b](http://latex.codecogs.com/svg.latex?b) is the imaginary part, with \
![i](http://latex.codecogs.com/svg.latex?i%3D%5Csqrt%7B-1%7D).\n";
        let out = apply(src);
        assert!(matches!(out, Cow::Owned(_)));

        let doc = ast::Document::parse(&out);
        let mut tex: Vec<String> = Vec::new();
        collect_math(doc.blocks(), &mut tex);
        assert_eq!(tex, ["a + ib", "a", "b", "i=\\sqrt{-1}"]);
        // And nothing of the prose was eaten on the way past.
        assert!(out.contains("is the real part and"));
        assert!(out.ends_with(".\n"));
    }

    fn collect_math(blocks: &[ast::Block], out: &mut Vec<String>) {
        for block in blocks {
            if let ast::BlockKind::Paragraph { children } = &block.kind {
                for inline in children {
                    if let ast::InlineKind::Math { tex, .. } = &inline.kind {
                        out.push(tex.clone());
                    }
                }
            }
        }
    }

    /// A `$` in the decoded formula would close the span early and spill the
    /// rest of the equation into the paragraph as prose. There is no safe
    /// inline form for it, so the image survives instead.
    #[test]
    fn test_a_formula_containing_a_dollar_is_left_as_an_image() {
        let src = "![m](http://latex.codecogs.com/svg.latex?%24x%24)\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
        assert_eq!(decode_tex("%24x%24"), None);
        assert_eq!(decode_tex(""), None);
        assert_eq!(decode_tex("%20%20"), None);
    }

    #[test]
    fn test_a_codecogs_url_inside_a_fence_is_not_rewritten() {
        let src = "```markdown\n![x](http://latex.codecogs.com/svg.latex?x)\n```\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    /// R10 at this pass: a document caught mid-save must come back byte for
    /// byte, even though the fence it is missing has an eligible image after
    /// it.
    #[test]
    fn test_an_unclosed_fence_leaves_the_whole_document_untouched() {
        let src = "```\nhalf a save\n\n![x](http://latex.codecogs.com/svg.latex?x)\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_a_malformed_percent_escape_does_not_panic_or_invent_bytes() {
        assert_eq!(decode_tex("a%").as_deref(), Some("a%"));
        assert_eq!(decode_tex("a%Z1b").as_deref(), Some("a%Z1b"));
        assert_eq!(decode_tex("a%2").as_deref(), Some("a%2"));
        // A percent escape that decodes to invalid UTF-8 is refused whole.
        assert_eq!(decode_tex("%FF%FE"), None);
    }

    #[test]
    fn test_crlf_line_endings_survive_the_rewrite() {
        let src = "![x](http://latex.codecogs.com/svg.latex?x)\r\nnext\r\n";
        let out = apply(src);
        assert_eq!(&*out, "$x$\r\nnext\r\n");
    }

    /// `mermaid.rs:280`'s drift test, in the shape this pass needs: what the
    /// module *claims* about the text and what the parser then makes of it
    /// must not come apart. Every case is checked against the real
    /// `Document`, so text that merely looks like maths — but that the
    /// parser reads as a paragraph, or as an image still pointing at
    /// codecogs.com — fails here rather than on a reader's screen.
    #[test]
    fn test_a_rewritten_document_holds_maths_and_an_untouched_one_holds_its_image() {
        // (source, how many `Math` inlines, how many surviving codecogs images)
        let cases = [
            ("plain\n", 0, 0),
            ("![x](http://latex.codecogs.com/svg.latex?x%5E2)\n", 1, 0),
            (
                "![x](https://latex.codecogs.com/png.image?a&plus;b)\n",
                1,
                0,
            ),
            // Declined: inside a fence, `$`-bearing, and mid-save.
            (
                "```\n![x](http://latex.codecogs.com/svg.latex?x)\n```\n",
                0,
                0,
            ),
            ("![x](http://latex.codecogs.com/svg.latex?%24x%24)\n", 0, 1),
            ("```\n![x](http://latex.codecogs.com/svg.latex?x)\n", 0, 0),
        ];
        for (src, maths, images) in cases {
            let out = apply(src);
            let doc = ast::Document::parse(&out);
            let mut tex = Vec::new();
            collect_math(doc.blocks(), &mut tex);
            assert_eq!(
                tex.len(),
                maths,
                "math count for {src:?} (text was {out:?})"
            );
            assert_eq!(
                count_codecogs_images(doc.blocks()),
                images,
                "surviving images for {src:?}"
            );
            if maths == 0 && images == 0 {
                assert!(
                    matches!(out, Cow::Borrowed(_)),
                    "nothing was rewritten, so {src:?} must come back borrowed"
                );
            }
        }
    }

    fn count_codecogs_images(blocks: &[ast::Block]) -> usize {
        let mut found = 0;
        for block in blocks {
            if let ast::BlockKind::Paragraph { children } = &block.kind {
                for inline in children {
                    if let ast::InlineKind::Image { dest, .. } = &inline.kind
                        && dest.contains("codecogs")
                    {
                        found += 1;
                    }
                }
            }
        }
        found
    }
}
