//! Owned kitty graphics-protocol emission: chunked base64 transmission,
//! direct-mode placement, and deletion.
//!
//! **Placement mode.** This emits *direct* placement (`a=t`/`a=p` at the
//! cursor, sized in cells), not virtual placement (`U=1` + Unicode
//! placeholders) — see the crate-level doc comment for why: the spike's own
//! verdict on `U=1` was sequence-accepted but visually unverified, and no
//! GUI session was available here to run the manual pass it asked for
//! before treating `U=1` as load-bearing.
//!
//! Every response from the terminal is suppressed (`q=2`): the caller
//! writes fire-and-forget, matching [`crate`]'s pinned `Result`-less
//! signatures, and it means kitty's APC replies never interleave with a
//! keyboard-event reader on the same fd (Spike A confirmed the two
//! coexist on the wire; suppressing kitty's replies avoids needing to
//! parse and discard them at all).

use std::io::Write;

use base64::Engine as _;

/// Each kitty transmission chunk carries at most this many base64 bytes
/// (protocol limit; see `docs/spikes/ghostty-caps.md` item 2).
const CHUNK_SIZE: usize = 4096;

/// Identity of one kitty-protocol image. Kitty's own id space is a plain
/// `u32`; `0` is reserved by the protocol to mean "no id," so callers should
/// allocate from `1..`, but this type does not itself enforce that (the
/// caller — [`crate`]'s `MediaSink` — owns id allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(u32);

impl ImageId {
    pub fn new(id: u32) -> Self {
        ImageId(id)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for ImageId {
    fn from(id: u32) -> Self {
        ImageId(id)
    }
}

/// A cell rectangle to place an image into: `(x, y)` is the top-left cell
/// (0-indexed, column then row), `width`/`height` the extent in cells.
/// Defined locally rather than reusing `stele::painter::CellRect` — this
/// crate does not depend on `stele` (the dependency runs the other way);
/// the two types are structurally identical by design and the `MediaSink`
/// impl converts between them at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Emits kitty graphics-protocol escape sequences. Holds no state of its
/// own — every method is a pure encode-and-write of its arguments; the
/// caller (`MediaSink`) owns image-id allocation, the eviction policy, and
/// the create/delete emission log.
#[derive(Debug, Default, Clone, Copy)]
pub struct Emitter;

impl Emitter {
    pub fn new() -> Self {
        Emitter
    }

    /// Transmits `png` (a complete PNG file's bytes) as kitty image `id`,
    /// format `f=100` (kitty decodes the PNG itself), chunked into
    /// `<= 4096`-base64-byte pieces per the protocol's own chunking rule.
    ///
    /// Handles the two edge cases a naive `payload.chunks(N)` over the
    /// *encoded* string gets right by construction, and are worth stating
    /// explicitly because they are easy to get wrong: a zero-length `png`
    /// still emits exactly one (empty-payload) chunk — so the terminal
    /// always receives a well-formed `m=0` final chunk rather than silently
    /// no transmission at all — and a payload whose length isn't a multiple
    /// of 3 (the overwhelming common case) never risks a corrupt split,
    /// because the full input is base64-encoded *before* chunking: the
    /// padding characters that a non-multiple-of-3 tail produces land
    /// wherever they land in the final chunk, and kitty concatenates every
    /// chunk's payload text before base64-decoding the whole, so an
    /// arbitrary byte-offset split of pure-ASCII base64 text is always
    /// safe.
    ///
    /// Every reply from the terminal is suppressed (`q=2`; see the module
    /// doc comment). Write failures are swallowed: the pinned signature
    /// returns nothing, matching `MediaSink::paint`'s own fire-and-forget
    /// contract one layer up.
    pub fn transmit(&mut self, id: ImageId, png: &[u8], out: &mut dyn Write) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let chunks = chunk_base64(&encoded);
        let last = chunks.len() - 1;
        for (i, chunk) in chunks.iter().enumerate() {
            let more = if i == last { 0 } else { 1 };
            if i == 0 {
                let _ = write!(
                    out,
                    "\x1b_Ga=t,f=100,i={},m={more},q=2;{chunk}\x1b\\",
                    id.get()
                );
            } else {
                let _ = write!(out, "\x1b_Gm={more};{chunk}\x1b\\");
            }
        }
    }

    /// Places already-transmitted image `id` at `rect`: moves the cursor to
    /// `rect`'s top-left cell (`CSI row;colH`, 1-indexed), then emits a put
    /// command (`a=p`) sized to `rect` in cells (`c=`/`r=`) — kitty scales
    /// the source raster to fit the requested cell box. This is the direct-
    /// placement half of "repaint-behind": the caller re-issues `place` (or
    /// `delete` + a fresh `transmit`/`place`) every frame a Reserved box is
    /// visible, so painting the surrounding text on top of/around it is
    /// exactly the same full-repaint model the rest of the painter uses.
    ///
    /// **Placement id (`p=`) is load-bearing.** The put carries an explicit,
    /// stable placement id equal to the image id, because stele keeps exactly
    /// one placement per image. Per the kitty graphics protocol, "if you send
    /// two placements with the same `image id` and `placement id` the second
    /// one will replace the first" — so re-placing on the next frame *moves*
    /// the image to its new row without flicker. Omitting `p=` (or `p=0`)
    /// does the opposite: the spec states it "results in multiple placements
    /// [of] the image," so every scroll frame would leave a stale copy behind
    /// — the exact vertical trail of ghost images the repaint-behind model is
    /// meant to avoid. The one-placement-per-image invariant makes reusing the
    /// image id as the placement id both safe and sufficient.
    /// **Cursor policy (`C=1`) is load-bearing too.** By default a put leaves
    /// the cursor after the image. Both of this painter's placement paths are
    /// hostile to that:
    ///
    /// - An *inline* box shares its row with text, so the run after it has to
    ///   resume at a known column.
    /// - A *block* box is followed immediately by `CSI K` (clear to end of
    ///   line). If the put has already walked the cursor onto a later row,
    ///   that erases part of a row the painter has already drawn — and a
    ///   placement on the last visible row would push the cursor past the
    ///   bottom, scrolling the whole frame out from under the viewer.
    ///
    /// `C=1` pins the cursor at the placement origin, so the caller keeps
    /// full control of where the next byte lands.
    pub fn place(&mut self, id: ImageId, rect: CellRect, out: &mut dyn Write) {
        let row = rect.y.saturating_add(1);
        let col = rect.x.saturating_add(1);
        let _ = write!(out, "\x1b[{row};{col}H");
        let _ = write!(
            out,
            "\x1b_Ga=p,i={},p={},c={},r={},C=1,q=2\x1b\\",
            id.get(),
            id.get(),
            rect.width.max(1),
            rect.height.max(1)
        );
    }

    /// Deletes image `id`: its placements *and* its stored pixel data
    /// (`a=d,d=I`). Per `docs/spikes/ghostty-caps.md` item 4, Ghostty does not
    /// acknowledge this — callers must track create/delete balance from their
    /// own send-log, never from a terminal-side reply.
    ///
    /// The uppercase `d=I` matters: lowercase `d=i` removes placements but
    /// leaves the transmitted pixel data resident in the terminal. Because an
    /// evicted node is re-transmitted under a *fresh* id when it scrolls back
    /// into view, lowercase would accumulate one orphaned copy of every image
    /// per eviction for the life of the session, until the terminal hit its
    /// own storage quota and began silently rejecting new transmits.
    pub fn delete(&mut self, id: ImageId, out: &mut dyn Write) {
        let _ = write!(out, "\x1b_Ga=d,d=I,i={},q=2\x1b\\", id.get());
    }

    /// Deletes image `id`'s **placements only** (`a=d,d=i` — lowercase),
    /// leaving its transmitted pixel data resident so an immediately
    /// following [`place`](Self::place) can redraw it without re-transmitting.
    ///
    /// This is the backstop for the repaint-behind model when the terminal
    /// does not honor same-`(i, p)` put replacement: emitting this before each
    /// re-place guarantees the previous frame's placement is gone before the
    /// new one is drawn, so a still-visible image cannot accumulate a stacked
    /// trail regardless of whether the terminal treats a repeated put as a
    /// move or as a new placement. Kept distinct from [`delete`](Self::delete)
    /// (`d=I`, which also frees the pixel data) precisely because the per-frame
    /// path must NOT drop the raster it is about to reuse.
    pub fn clear_placements(&mut self, id: ImageId, out: &mut dyn Write) {
        let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", id.get());
    }
}

/// Splits an already-base64-encoded (pure-ASCII) string into
/// `<= CHUNK_SIZE`-byte pieces, always yielding at least one chunk (an
/// empty string yields one empty chunk) so [`Emitter::transmit`] always
/// emits a well-formed final chunk even for a zero-length payload.
fn chunk_base64(encoded: &str) -> Vec<&str> {
    if encoded.is_empty() {
        return vec![""];
    }
    // Safe to split by byte offset: every base64 alphabet character
    // (including the `=` padding) is a single ASCII byte, so no chunk
    // boundary can land inside a multi-byte character.
    encoded
        .as_bytes()
        .chunks(CHUNK_SIZE)
        .map(|c| std::str::from_utf8(c).expect("base64 output is pure ASCII"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(f: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut buf = Vec::new();
        f(&mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_chunk_base64_zero_length_yields_one_empty_chunk() {
        assert_eq!(chunk_base64(""), vec![""]);
    }

    #[test]
    fn test_chunk_base64_splits_at_exact_boundary() {
        let s = "a".repeat(CHUNK_SIZE * 2);
        let chunks = chunk_base64(&s);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), CHUNK_SIZE);
        assert_eq!(chunks[1].len(), CHUNK_SIZE);
    }

    #[test]
    fn test_chunk_base64_non_multiple_of_chunk_size_tail_is_shorter() {
        let s = "a".repeat(CHUNK_SIZE + 10);
        let chunks = chunk_base64(&s);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), CHUNK_SIZE);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks.concat(), s);
    }

    #[test]
    fn test_transmit_zero_length_png_emits_single_well_formed_chunk() {
        let out = captured(|buf| Emitter::new().transmit(ImageId::new(7), &[], buf));
        assert_eq!(out, "\x1b_Ga=t,f=100,i=7,m=0,q=2;\x1b\\");
    }

    #[test]
    fn test_transmit_non_multiple_of_3_payload_round_trips_through_chunking() {
        // 16385 bytes: not a multiple of 3, and large enough to force a
        // second chunk once base64-encoded (base64 expands length by ~4/3).
        let payload = vec![0xABu8; 16385];
        let out = captured(|buf| Emitter::new().transmit(ImageId::new(1), &payload, buf));
        assert!(out.starts_with("\x1b_Ga=t,f=100,i=1,m=1,q=2;"));
        assert!(out.contains("\x1b_Gm=0;"));
        // Re-extract every payload segment and confirm the concatenation
        // decodes back to the original bytes, proving the chunk split
        // never corrupted the base64 stream.
        let mut encoded = String::new();
        for segment in out.split("\x1b_G").skip(1) {
            let body = segment.trim_end_matches("\x1b\\");
            if let Some(idx) = body.find(';') {
                encoded.push_str(&body[idx + 1..]);
            }
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_place_moves_cursor_and_emits_put_sized_to_rect() {
        let rect = CellRect {
            x: 4,
            y: 2,
            width: 10,
            height: 3,
        };
        let out = captured(|buf| Emitter::new().place(ImageId::new(9), rect, buf));
        assert_eq!(out, "\x1b[3;5H\x1b_Ga=p,i=9,p=9,c=10,r=3,C=1,q=2\x1b\\");
    }

    /// `C=1` pins the cursor at the placement origin. Dropping it lets the
    /// put walk the cursor past the image, which corrupts an inline box's
    /// line (the following run resumes at the wrong column) and, on the last
    /// visible row, scrolls the whole frame. Asserted on its own so the
    /// reason survives even if the byte-exact test above is ever relaxed.
    #[test]
    fn test_place_pins_the_cursor_so_it_never_walks_past_the_image() {
        let out = captured(|buf| {
            Emitter::new().place(
                ImageId::new(3),
                CellRect {
                    x: 12,
                    y: 0,
                    width: 6,
                    height: 1,
                },
                buf,
            )
        });
        assert!(out.contains("C=1"), "cursor must not move: {out:?}");
    }

    /// Regression for the ghost-image trail: re-placing the same image at a
    /// new row on a later frame must reuse the SAME `(i, p)` pair so kitty
    /// replaces (moves) the placement instead of stacking a second one. A
    /// missing or `0` placement id silently reintroduces the trail, so this
    /// asserts a stable, nonzero `p=` equal to the image id on every place.
    #[test]
    fn test_place_emits_stable_placement_id_so_replace_moves_not_stacks() {
        let frame1 = captured(|buf| {
            Emitter::new().place(
                ImageId::new(7),
                CellRect {
                    x: 0,
                    y: 2,
                    width: 8,
                    height: 4,
                },
                buf,
            )
        });
        let frame2 = captured(|buf| {
            Emitter::new().place(
                ImageId::new(7),
                CellRect {
                    x: 0,
                    y: 5,
                    width: 8,
                    height: 4,
                },
                buf,
            )
        });
        assert!(
            frame1.contains("i=7,p=7,"),
            "frame 1 missing stable p=: {frame1:?}"
        );
        assert!(
            frame2.contains("i=7,p=7,"),
            "frame 2 missing stable p=: {frame2:?}"
        );
        // The put commands are identical apart from the cursor move — same
        // (i, p) pair, so kitty treats frame 2 as a move of frame 1's
        // placement, not a new one.
        let put1 = &frame1[frame1.find("\x1b_G").unwrap()..];
        let put2 = &frame2[frame2.find("\x1b_G").unwrap()..];
        assert_eq!(put1, put2, "put command must be identical across frames");
    }

    #[test]
    fn test_place_clamps_zero_extent_to_at_least_one_cell() {
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let out = captured(|buf| Emitter::new().place(ImageId::new(1), rect, buf));
        assert!(out.contains("c=1,r=1"));
    }

    #[test]
    fn test_delete_emits_a_d_d_i() {
        let out = captured(|buf| Emitter::new().delete(ImageId::new(3), buf));
        assert_eq!(out, "\x1b_Ga=d,d=I,i=3,q=2\x1b\\");
    }

    #[test]
    fn test_clear_placements_emits_lowercase_d_i_keeping_pixel_data() {
        let out = captured(|buf| Emitter::new().clear_placements(ImageId::new(3), buf));
        // Lowercase d=i: placements gone, transmitted raster kept for reuse.
        assert_eq!(out, "\x1b_Ga=d,d=i,i=3,q=2\x1b\\");
    }
}
