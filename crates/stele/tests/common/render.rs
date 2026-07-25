//! A model of the terminal's cell grid, driven only by the bytes the painter
//! emitted.
//!
//! Why a model at all: the defects this suite was written for are *positional*.
//! "The fallback text is on the wire" was true while the fallback was invisible,
//! because the painter homed back over it a few bytes later. Only replaying the
//! frame the way a terminal would — honour CUP, honour EL(0), otherwise write
//! glyphs left to right — can answer "what does the reader see on row 3".
//!
//! Two copies of this model used to exist. The one kept is the one that skips
//! kitty graphics payloads: without that, a frame carrying a real raster writes
//! its base64 into the cell grid as text and every row assertion goes strange.
//! The discarded copy was safe only because every test using it happened to
//! force both graphics rungs to fail — an accident, not a property.

/// Replays `frame`'s bytes and returns 1-indexed `row` as the terminal would
/// show it, `width` cells wide, space-padded.
pub fn render_row(frame: &str, row: u16, width: usize) -> String {
    let mut grid = vec![' '; width];
    let mut cur_row: u16 = 1;
    let mut col: usize = 0;
    let chars: Vec<char> = frame.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\u{1b}' {
            // Skip an APC (kitty graphics) payload wholesale: it is not text.
            if chars.get(i + 1) == Some(&'_') {
                while i < chars.len() && chars[i] != '\u{9c}' {
                    if chars[i] == '\u{1b}' && chars.get(i + 1) == Some(&'\\') && i > 0 {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            let mut j = i + 2; // skip ESC [
            while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            let params: String = chars[i + 2..j].iter().collect();
            match chars.get(j) {
                Some('H') => {
                    let mut it = params.split(';');
                    cur_row = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                    col = it
                        .next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .saturating_sub(1);
                }
                Some('K') if cur_row == row => {
                    for c in grid.iter_mut().skip(col) {
                        *c = ' ';
                    }
                }
                _ => {}
            }
            i = j + 1;
            continue;
        }
        if cur_row == row && col < width {
            grid[col] = chars[i];
        }
        col += 1;
        i += 1;
    }
    grid.into_iter().collect()
}
