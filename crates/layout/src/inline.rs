//! Inline flattening and greedy wrapping.
//!
//! An inline tree flattens into a sequence of [`Atom`]s: unbreakable word
//! groups of styled fragments, collapsed spaces, hard breaks, and (when the
//! sizer speaks) reserved media boxes. Wrapping is greedy over those atoms;
//! every measurement goes through the [`WidthEngine`] over grapheme
//! clusters — never byte or char counts. A word group wider than the
//! content width falls back to break-anywhere at cluster boundaries.

use std::rc::Rc;

use ast::{Document, Inline, InlineKind};
use width::{WidthEngine, graphemes};

use crate::{CellSize, IntrinsicSizer, Run, Semantic, StyleId};

/// The clip indicator (code blocks, table rung 3, break-anywhere guard).
pub(crate) const INDICATOR: &str = "\u{2026}"; // …

/// One styled fragment inside a word group.
#[derive(Debug)]
pub(crate) struct Frag {
    pub text: String,
    pub style: Semantic,
    /// Destination URL when this fragment is inside a link (`Semantic::Link`),
    /// carried onto the emitted run's `aux` so the painter can wrap it in an
    /// OSC 8 hyperlink. `Rc` because every fragment of one link shares it.
    pub link: Option<Rc<str>>,
}

impl Frag {
    fn link_box(&self) -> Option<Box<str>> {
        self.link.as_ref().map(|s| Box::from(&**s))
    }
}

/// One wrappable unit.
#[derive(Debug)]
pub(crate) enum Atom {
    /// An unbreakable group of styled fragments (style changes are not
    /// break opportunities).
    Word(Vec<Frag>),
    /// A collapsed run of whitespace / a soft break.
    Space,
    HardBreak,
    /// A sizer-sized media box (image or math), already at natural size.
    Box(ast::NodeId, CellSize),
}

/// One wrapped output line or an interleaved media box.
#[derive(Debug)]
pub(crate) enum Piece {
    Runs(Vec<Run>),
    Box(ast::NodeId, CellSize),
}

/// Flatten an inline tree under `base` style. `allow_box` gates the sizer:
/// table cells pass `false` so media inside cells degrades to text.
pub(crate) fn flatten(
    inlines: &[Inline],
    base: Semantic,
    doc: &Document,
    sizer: &dyn IntrinsicSizer,
    allow_box: bool,
) -> Vec<Atom> {
    let mut fl = Flattener {
        doc,
        sizer,
        allow_box,
        atoms: Vec::new(),
        word: Vec::new(),
        link: None,
    };
    fl.visit_all(inlines, base);
    fl.flush_word();
    fl.atoms
}

struct Flattener<'a> {
    doc: &'a Document,
    sizer: &'a dyn IntrinsicSizer,
    allow_box: bool,
    atoms: Vec<Atom>,
    word: Vec<Frag>,
    /// The enclosing link's destination while visiting its children, so text
    /// fragments inside it carry the URL. Save/restore around the subtree.
    link: Option<Rc<str>>,
}

impl Flattener<'_> {
    fn visit_all(&mut self, inlines: &[Inline], style: Semantic) {
        for inline in inlines {
            self.visit(inline, style);
        }
    }

    /// Innermost-role-wins: entering a styled container replaces the
    /// current role for its children.
    fn visit(&mut self, inline: &Inline, style: Semantic) {
        match &inline.kind {
            InlineKind::Text(s) => self.push_text(s, style),
            InlineKind::SoftBreak => self.push_space(),
            InlineKind::HardBreak => {
                self.flush_word();
                // A pending space before a forced break is moot.
                if matches!(self.atoms.last(), Some(Atom::Space)) {
                    self.atoms.pop();
                }
                self.atoms.push(Atom::HardBreak);
            }
            InlineKind::Code(s) => self.push_text(s, Semantic::CodeInline),
            InlineKind::HtmlInline(s) => self.push_text(s, Semantic::Html),
            InlineKind::Emph(children) => self.visit_all(children, Semantic::Emph),
            InlineKind::Strong(children) => self.visit_all(children, Semantic::Strong),
            InlineKind::Strikethrough(children) => {
                self.visit_all(children, Semantic::Strikethrough)
            }
            InlineKind::Link { children, dest, .. } => {
                // Carry the raw destination while visiting the link's text;
                // the painter sanitizes it before it reaches an OSC 8
                // sequence. Save/restore handles the (rare) nested case.
                let saved = self.link.take();
                self.link = Some(Rc::from(dest.as_str()));
                self.visit_all(children, Semantic::Link);
                self.link = saved;
            }
            InlineKind::Image { children, .. } => match self.try_size(inline) {
                Some(size) => self.push_box(inline, size),
                None => self.visit_all(children, Semantic::ImageAlt),
            },
            InlineKind::Math { tex, .. } => match self.try_size(inline) {
                Some(size) => self.push_box(inline, size),
                None => self.push_text(tex, Semantic::MathTex),
            },
            InlineKind::FootnoteReference { label } => {
                self.push_text(&format!("[^{label}]"), Semantic::FootnoteRef)
            }
        }
    }

    fn try_size(&self, inline: &Inline) -> Option<CellSize> {
        if !self.allow_box {
            return None;
        }
        self.sizer.size(inline.id, self.doc)
    }

    fn push_box(&mut self, inline: &Inline, size: CellSize) {
        self.flush_word();
        self.atoms.push(Atom::Box(inline.id, size));
    }

    /// Split text at whitespace; whitespace runs collapse to one `Space`.
    /// Non-space content appends to the open word group so words spanning
    /// style boundaries stay unbreakable.
    fn push_text(&mut self, s: &str, style: Semantic) {
        for ch in s.chars() {
            if ch.is_whitespace() {
                self.flush_word();
                self.push_space();
            } else {
                match self.word.last_mut() {
                    Some(f) if f.style == style && f.link == self.link => f.text.push(ch),
                    _ => self.word.push(Frag {
                        text: ch.to_string(),
                        style,
                        link: self.link.clone(),
                    }),
                }
            }
        }
    }

    fn push_space(&mut self) {
        self.flush_word();
        if !matches!(self.atoms.last(), Some(Atom::Space) | None) {
            self.atoms.push(Atom::Space);
        }
    }

    fn flush_word(&mut self) {
        if !self.word.is_empty() {
            self.atoms.push(Atom::Word(std::mem::take(&mut self.word)));
        }
    }
}

/// Greedy-wrap `atoms` into lines of at most `content_width` cells.
pub(crate) fn wrap(atoms: Vec<Atom>, content_width: u16, engine: &WidthEngine) -> Vec<Piece> {
    let cw = content_width.max(1);
    let mut out: Vec<Piece> = Vec::new();
    let mut cur: Vec<Run> = Vec::new();
    let mut cur_w: u16 = 0;
    let mut pending_space = false;

    let flush = |out: &mut Vec<Piece>, cur: &mut Vec<Run>, cur_w: &mut u16| {
        out.push(Piece::Runs(std::mem::take(cur)));
        *cur_w = 0;
    };

    for atom in atoms {
        match atom {
            Atom::Space => pending_space = !cur.is_empty(),
            Atom::HardBreak => {
                flush(&mut out, &mut cur, &mut cur_w);
                pending_space = false;
            }
            Atom::Box(node, size) => {
                if !cur.is_empty() {
                    flush(&mut out, &mut cur, &mut cur_w);
                }
                out.push(Piece::Box(node, size));
                pending_space = false;
            }
            Atom::Word(frags) => {
                let word_w: u16 = frags
                    .iter()
                    .fold(0u16, |acc, f| acc.saturating_add(cells(engine, &f.text)));
                let space_w = u16::from(pending_space && !cur.is_empty());
                if !cur.is_empty() && cur_w.saturating_add(space_w).saturating_add(word_w) > cw {
                    flush(&mut out, &mut cur, &mut cur_w);
                } else if space_w == 1 {
                    let style = StyleId::Semantic(frags[0].style);
                    append(&mut cur, " ", style, 1);
                    cur_w += 1;
                }
                if word_w > cw {
                    // Break-anywhere fallback: the word cannot fit any line.
                    break_anywhere(&mut out, &mut cur, &mut cur_w, cw, &frags, engine);
                } else {
                    for f in &frags {
                        let w = cells(engine, &f.text);
                        append_aux(
                            &mut cur,
                            &f.text,
                            StyleId::Semantic(f.style),
                            w,
                            f.link_box(),
                        );
                    }
                    cur_w = cur_w.saturating_add(word_w);
                }
                pending_space = false;
            }
        }
    }
    if !cur.is_empty() {
        out.push(Piece::Runs(cur));
    }
    out
}

/// Fill lines cluster by cluster (used when a word group exceeds the full
/// content width). A single cluster wider than the line itself (only
/// possible at pathological widths of 1) degrades to the indicator.
fn break_anywhere(
    out: &mut Vec<Piece>,
    cur: &mut Vec<Run>,
    cur_w: &mut u16,
    cw: u16,
    frags: &[Frag],
    engine: &WidthEngine,
) {
    for f in frags {
        let style = StyleId::Semantic(f.style);
        let link = f.link_box();
        for g in graphemes(&f.text) {
            let gw = engine.cluster_width(g);
            if gw > cw {
                let iw = cells(engine, INDICATOR);
                if cur_w.saturating_add(iw) > cw {
                    out.push(Piece::Runs(std::mem::take(cur)));
                    *cur_w = 0;
                }
                if iw <= cw {
                    append(
                        cur,
                        INDICATOR,
                        StyleId::Semantic(Semantic::OverflowIndicator),
                        iw,
                    );
                    *cur_w = cur_w.saturating_add(iw);
                }
                continue;
            }
            if cur_w.saturating_add(gw) > cw {
                out.push(Piece::Runs(std::mem::take(cur)));
                *cur_w = 0;
            }
            append_aux(cur, g, style, gw, link.clone());
            *cur_w = cur_w.saturating_add(gw);
        }
    }
}

/// Cell width of `s`, saturated at `u16::MAX`. Width math that decides
/// wrapping or clipping must never let a too-wide token wrap around to a
/// small `u16` — that would slip an over-wide run past a fit check and emit
/// a line wider than the viewport while its `Run.width` lies about it. Every
/// run that survives to the tree is ≤ the line budget, so this ceiling is
/// only reachable by pathological single-token input, which the wrap and
/// clip paths then break or clip cluster by cluster.
pub(crate) fn cells(engine: &WidthEngine, s: &str) -> u16 {
    engine.display_width(s).min(u16::MAX as usize) as u16
}

/// Append text to the line, merging into the last run when style matches.
pub(crate) fn append(cur: &mut Vec<Run>, text: &str, style_id: StyleId, width: u16) {
    append_aux(cur, text, style_id, width, None);
}

/// Like [`append`], but carries `aux` (the link URL) onto the run. Merges
/// into the last run only when BOTH style and `aux` match, so two adjacent
/// links with different destinations never coalesce into one hyperlink.
pub(crate) fn append_aux(
    cur: &mut Vec<Run>,
    text: &str,
    style_id: StyleId,
    width: u16,
    aux: Option<Box<str>>,
) {
    if text.is_empty() {
        return;
    }
    match cur.last_mut() {
        Some(run) if run.style_id == style_id && run.aux == aux => {
            run.text.push_str(text);
            // Saturate: a merged run past u16::MAX is pathological, but
            // clip_runs re-measures by cluster, so an honest-but-capped
            // width still triggers clipping correctly.
            run.width = run.width.saturating_add(width);
        }
        _ => cur.push(Run {
            text: text.to_string(),
            style_id,
            width,
            aux,
        }),
    }
}

/// Clip `runs` to at most `max_width` cells at cluster boundaries. With
/// `indicate`, the last cells make room for the `…` overflow indicator.
/// Returns the runs untouched when they already fit.
pub(crate) fn clip_runs(
    runs: Vec<Run>,
    max_width: u16,
    indicate: bool,
    engine: &WidthEngine,
) -> Vec<Run> {
    let total: usize = runs.iter().map(|r| r.width as usize).sum();
    if total <= max_width as usize {
        return runs;
    }
    let iw = if indicate {
        engine.display_width(INDICATOR) as u16
    } else {
        0
    };
    let target = max_width.saturating_sub(iw);
    let mut out: Vec<Run> = Vec::new();
    let mut w: u16 = 0;
    for run in runs {
        if w.saturating_add(run.width) <= target {
            w += run.width;
            out.push(run);
            continue;
        }
        // The cut lands inside this run: keep whole clusters that fit.
        let mut text = String::new();
        let mut tw: u16 = 0;
        for g in graphemes(&run.text) {
            let gw = engine.cluster_width(g);
            if w.saturating_add(tw).saturating_add(gw) > target {
                break;
            }
            text.push_str(g);
            tw += gw;
        }
        append_aux(&mut out, &text, run.style_id, tw, run.aux.clone());
        break;
    }
    if indicate && iw <= max_width {
        append(
            &mut out,
            INDICATOR,
            StyleId::Semantic(Semantic::OverflowIndicator),
            iw,
        );
    }
    out
}
