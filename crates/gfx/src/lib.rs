//! `crates/gfx` — owned kitty graphics-protocol emission, plus bounded local
//! image decode.
//!
//! Two independent concerns share this crate because they are two halves of
//! the same job ("get a raster of known pixel dimensions onto the Ghostty
//! screen"): [`protocol`] emits the wire bytes for a PNG payload of any
//! origin (a decoded photo, a RaTeX-rendered formula); [`decode`] turns an
//! untrusted local image file into that PNG payload safely, bounded so a
//! decompression bomb cannot exhaust memory.
//!
//! **Placement-mode deviation, recorded honestly:** `docs/spikes/ghostty-caps.md`
//! item 3 found virtual placement (`U=1` + Unicode placeholders) "accepted
//! at the sequence level — visual placement not independently confirmed,"
//! and explicitly asked P6 to run one manual visual pass before treating it
//! as load-bearing. No interactive GUI session was available to run that
//! pass in this build. Per the spike's own stated fallback, [`Emitter`]
//! implements **direct placement + repaint-behind** (`a=t`/`a=p` at the
//! cursor, sized in cells via `c=`/`r=`), not virtual placement.
//!
//! `#![forbid(unsafe_code)]`: every input this crate touches (an image file
//! path from a markdown document, in the decode path) is hostile until
//! proven otherwise.

#![forbid(unsafe_code)]

pub mod decode;
pub mod protocol;

pub use decode::{DecodeError, DecodedImage, Limits};
pub use protocol::{CellRect, Emitter, ImageId, SourceRect};
