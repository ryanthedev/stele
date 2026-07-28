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

use crate::{CellSize, HeadingCase, IntrinsicSizer, LineItem, Reserved, Run, Semantic, StyleId};

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

/// Whether a media box is allowed to share a line with text.
///
/// Size alone is the wrong test. `$$E=mc^2$$` measures one row, but display
/// math is *semantically* a block — the author wrote `$$` precisely to set it
/// apart from the prose — so letting it ride the baseline because it happens
/// to be short would silently rewrite what the document says. Inline `$…$`
/// math and images carry no such intent and flow with the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxFlow {
    /// May ride the baseline when it fits.
    Inline,
    /// Always claims its own rows, whatever it measures.
    Block,
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
    Box(ast::NodeId, CellSize, BoxFlow),
}

/// Small words that stay lowercase in title case unless they are the first
/// or last word of the heading — the ordinary typographic rule, so
/// "Installing the Toolchain" rather than "Installing The Toolchain".
const TITLE_SMALL_WORDS: &[&str] = &[
    "a", "an", "the", "and", "but", "or", "nor", "for", "so", "yet", "at", "by", "in", "of", "on",
    "to", "up", "via", "as", "is", "it", "with", "from", "into", "over",
];

/// Rewrite heading atoms to the case their level renders in.
///
/// This runs **before** [`wrap`], so widths are measured on the text that
/// actually gets painted. It has to: `ß`.to_uppercase() is `SS`, so casing
/// after measurement would let a heading overrun the measure it was wrapped
/// to.
///
/// Casing here rather than at paint time is what keeps search honest. Search
/// scans the rendered line text (`append_line_text` over the layout tree), so
/// if the painter did the casing, a reader looking at `SUPPORTED PLATFORMS`
/// could not search for it — they would have to guess the author's original
/// capitalisation. The outline and the TOC are unaffected either way: those
/// come from `heading_text` over the AST, which never passes through here, so
/// the sidebar keeps the author's own words.
pub(crate) fn recase(atoms: &mut [Atom], case: HeadingCase) {
    if case == HeadingCase::AsWritten {
        return;
    }
    let word_positions: Vec<usize> = atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a, Atom::Word(_)))
        .map(|(i, _)| i)
        .collect();
    let (first, last) = match (word_positions.first(), word_positions.last()) {
        (Some(&f), Some(&l)) => (f, l),
        _ => return,
    };
    for idx in word_positions {
        let Some(Atom::Word(frags)) = atoms.get_mut(idx) else {
            continue;
        };
        match case {
            HeadingCase::AsWritten => {}
            HeadingCase::Upper => {
                for frag in frags.iter_mut() {
                    frag.text = frag.text.to_uppercase();
                }
            }
            HeadingCase::Title => {
                // A word is split across fragments when a style changes mid
                // word, so "capitalise the first letter" means the first
                // letter of the *word*, not of each fragment.
                let whole: String = frags.iter().map(|f| f.text.as_str()).collect();
                let small = TITLE_SMALL_WORDS.iter().any(|w| {
                    whole
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .eq_ignore_ascii_case(w)
                });
                let keep_lower = small && idx != first && idx != last;
                let mut seen_alpha = false;
                for frag in frags.iter_mut() {
                    frag.text = recase_word(&frag.text, keep_lower, &mut seen_alpha);
                }
            }
        }
    }
}

/// One fragment of a title-cased word. `seen_alpha` carries across fragments
/// so only the word's genuine first letter is capitalised.
fn recase_word(text: &str, keep_lower: bool, seen_alpha: &mut bool) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_alphabetic() && !*seen_alpha {
            *seen_alpha = true;
            if keep_lower {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
        } else if ch.is_alphabetic() {
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// One wrapped output line, or a standalone media box that claims its own
/// rows. A box small enough to ride the baseline never becomes a
/// `Piece::Box` — it rides inside `Piece::Items` as a [`LineItem::Box`].
#[derive(Debug)]
pub(crate) enum Piece {
    Items(Vec<LineItem>),
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
                Some(size) => self.push_box(inline, size, BoxFlow::Inline),
                None => self.visit_all(children, Semantic::ImageAlt),
            },
            // `display` is the author's own statement of intent: `$$…$$` is set
            // apart from the prose, `$…$` runs through it. Honor it rather than
            // inferring flow from the rendered size.
            InlineKind::Math { tex, display } => {
                let flow = if *display {
                    BoxFlow::Block
                } else {
                    BoxFlow::Inline
                };
                match self.try_size(inline) {
                    Some(size) => self.push_box(inline, size, flow),
                    None => self.push_text(tex, Semantic::MathTex),
                }
            }
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

    fn push_box(&mut self, inline: &Inline, size: CellSize, flow: BoxFlow) {
        self.flush_word();
        self.atoms.push(Atom::Box(inline.id, size, flow));
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
    let mut cur: Vec<LineItem> = Vec::new();
    let mut cur_w: u16 = 0;
    let mut pending_space = false;

    let flush = |out: &mut Vec<Piece>, cur: &mut Vec<LineItem>, cur_w: &mut u16| {
        out.push(Piece::Items(std::mem::take(cur)));
        *cur_w = 0;
    };

    for atom in atoms {
        match atom {
            Atom::Space => pending_space = !cur.is_empty(),
            Atom::HardBreak => {
                flush(&mut out, &mut cur, &mut cur_w);
                pending_space = false;
            }
            // A one-row box narrow enough to share a line rides the baseline:
            // it wraps exactly like a word, keeping the spaces on either side,
            // so `text $formula$ more text` stays one flowing sentence. Only a
            // box that cannot do that falls through to the standalone arm.
            Atom::Box(node, size, BoxFlow::Inline) if size.rows <= 1 && size.cols.max(1) <= cw => {
                let box_w = size.cols.max(1);
                let space_w = u16::from(pending_space && !cur.is_empty());
                if !cur.is_empty() && cur_w.saturating_add(space_w).saturating_add(box_w) > cw {
                    flush(&mut out, &mut cur, &mut cur_w);
                } else if space_w == 1 {
                    append(&mut cur, " ", StyleId::Semantic(Semantic::Text), 1);
                    cur_w += 1;
                }
                cur.push(LineItem::Box(Reserved {
                    node_id: node,
                    cols: box_w,
                    rows: 1,
                    row: 0,
                }));
                cur_w = cur_w.saturating_add(box_w);
                pending_space = false;
            }
            // Taller than one row, wider than the whole content column, or
            // block by the author's own intent (`$$…$$`): it claims its own rows.
            Atom::Box(node, size, _) => {
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
        out.push(Piece::Items(cur));
    }
    out
}

/// Fill lines cluster by cluster (used when a word group exceeds the full
/// content width). A single cluster wider than the line itself (only
/// possible at pathological widths of 1) degrades to the indicator.
fn break_anywhere(
    out: &mut Vec<Piece>,
    cur: &mut Vec<LineItem>,
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
                    out.push(Piece::Items(std::mem::take(cur)));
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
                out.push(Piece::Items(std::mem::take(cur)));
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

/// The run-merging rule, in one place: fold `text` into `last` when BOTH
/// style and `aux` match (so two adjacent links with different destinations
/// never coalesce into one hyperlink), otherwise hand back the fresh run for
/// the caller to push. Shared by the `Vec<Run>` container (prefixes, table
/// cells) and the `Vec<LineItem>` container (a wrapped line, which may also
/// hold inline boxes).
fn merge_or_new(
    last: Option<&mut Run>,
    text: &str,
    style_id: StyleId,
    width: u16,
    aux: &Option<Box<str>>,
) -> Option<Run> {
    if let Some(run) = last
        && run.style_id == style_id
        && run.aux == *aux
    {
        run.text.push_str(text);
        // Saturate: a merged run past u16::MAX is pathological, but
        // clip_runs re-measures by cluster, so an honest-but-capped
        // width still triggers clipping correctly.
        run.width = run.width.saturating_add(width);
        return None;
    }
    Some(Run {
        text: text.to_string(),
        style_id,
        width,
        aux: aux.clone(),
    })
}

/// Append text to a run vector, merging into the last run when style matches.
pub(crate) fn append_run(cur: &mut Vec<Run>, text: &str, style_id: StyleId, width: u16) {
    append_run_aux(cur, text, style_id, width, None);
}

/// Like [`append_run`], but carries `aux` (the link URL) onto the run.
pub(crate) fn append_run_aux(
    cur: &mut Vec<Run>,
    text: &str,
    style_id: StyleId,
    width: u16,
    aux: Option<Box<str>>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(run) = merge_or_new(cur.last_mut(), text, style_id, width, &aux) {
        cur.push(run);
    }
}

/// Append text to a wrapped line, merging into the last run when style
/// matches.
pub(crate) fn append(cur: &mut Vec<LineItem>, text: &str, style_id: StyleId, width: u16) {
    append_aux(cur, text, style_id, width, None);
}

/// Like [`append`], but carries `aux` (the link URL) onto the run.
pub(crate) fn append_aux(
    cur: &mut Vec<LineItem>,
    text: &str,
    style_id: StyleId,
    width: u16,
    aux: Option<Box<str>>,
) {
    if text.is_empty() {
        return;
    }
    // An inline box ends the mergeable run: text following it starts a fresh
    // run rather than silently absorbing into the run that preceded the box.
    let last = match cur.last_mut() {
        Some(LineItem::Run(run)) => Some(run),
        _ => None,
    };
    if let Some(run) = merge_or_new(last, text, style_id, width, &aux) {
        cur.push(LineItem::Run(run));
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
        append_run_aux(&mut out, &text, run.style_id, tw, run.aux.clone());
        break;
    }
    if indicate && iw <= max_width {
        append_run(
            &mut out,
            INDICATOR,
            StyleId::Semantic(Semantic::OverflowIndicator),
            iw,
        );
    }
    out
}
