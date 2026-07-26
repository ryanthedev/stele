//! A model of the *terminal's* kitty-graphics state, rebuilt from the bytes a
//! frame actually emits.
//!
//! Lives here rather than in one test file because the questions it answers —
//! which images the terminal is holding, which it is *drawing*, and where —
//! are the ones every media test needs and the ones a count of escapes cannot
//! answer. Four green tests asserting create/delete balance hid four real
//! defects; the fix was to replay the wire, and a replayer worth trusting is
//! worth having exactly one of.

use std::collections::{BTreeMap, BTreeSet};

/// A model of the terminal's graphics state, driven only by the bytes the
/// painter emits.
///
/// Three questions the protocol can answer and a count cannot: which images
/// does the terminal hold, which is it *drawing*, and *where*. The row comes
/// from the `CSI row;colH` that `gfx::Emitter` emits immediately before every
/// put, so it is the terminal's own answer, not the painter's intent.
#[derive(Default, Debug)]
pub struct TerminalGfx {
    cursor: (u16, u16),
    /// Image ids whose pixel data the terminal is holding.
    pub stored: BTreeSet<u32>,
    /// Image id -> the 1-based `(row, col)` its live placement sits at.
    pub visible: BTreeMap<u32, (u16, u16)>,
}

impl TerminalGfx {
    /// Replays one frame's bytes; returns the ids that frame placed.
    pub fn apply(&mut self, frame: &[u8]) -> BTreeSet<u32> {
        let text = String::from_utf8_lossy(frame).into_owned();
        let bytes = text.as_bytes();
        let mut placed = BTreeSet::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] != 0x1b {
                i += 1;
                continue;
            }
            match bytes.get(i + 1) {
                // CSI: run to the final byte, and remember the cursor if it
                // was a CUP.
                Some(b'[') => {
                    let mut j = i + 2;
                    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'H' {
                        let mut params = text[i + 2..j].split(';');
                        let row = params.next().unwrap_or("").parse().unwrap_or(1);
                        let col = params.next().unwrap_or("").parse().unwrap_or(1);
                        self.cursor = (row, col);
                    }
                    i = j.saturating_add(1);
                }
                // APC: kitty graphics command, terminated by ESC \.
                Some(b'_') => {
                    let end = text[i..]
                        .find("\x1b\\")
                        .map(|o| i + o)
                        .unwrap_or(bytes.len());
                    let body = &text[i + 2..end];
                    let head = body.split(';').next().unwrap_or("");
                    let id = head
                        .split(',')
                        .find_map(|kv| kv.strip_prefix("i="))
                        .and_then(|v| v.parse::<u32>().ok());
                    if let Some(id) = id {
                        if head.starts_with("Ga=t") {
                            self.stored.insert(id);
                        } else if head.starts_with("Ga=p") {
                            assert!(
                                self.stored.contains(&id),
                                "placed image {id}, whose pixel data the terminal does not \
                                 have — the put draws nothing"
                            );
                            self.visible.insert(id, self.cursor);
                            placed.insert(id);
                        } else if head.starts_with("Ga=d,d=i") {
                            self.visible.remove(&id);
                        } else if head.starts_with("Ga=d,d=I") {
                            self.visible.remove(&id);
                            self.stored.remove(&id);
                        }
                    }
                    i = end.saturating_add(2);
                }
                _ => i += 2,
            }
        }
        placed
    }

    /// The 1-based rows the terminal is currently drawing an image on.
    pub fn visible_rows(&self) -> BTreeSet<u16> {
        self.visible.values().map(|&(row, _)| row).collect()
    }
}
