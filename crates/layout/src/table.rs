//! Table auto-layout: intrinsic min/max content widths per column,
//! css-tables-3 §3.9.3 width distribution (comfy-table's algorithm was
//! consulted as a reference; the implementation is owned), and the
//! overflow ladder in contract order:
//!
//! 1. **wrap-in-cell** — columns land between min- and max-content width;
//!    cells wrap at their assigned width;
//! 2. **per-column floors** — columns squeeze below min-content down to a
//!    floor; cell text takes the break-anywhere fallback naturally;
//! 3. **clip with indicator** — floors + separators still exceed the
//!    width, so each composed row line is clipped with `…`.
//!
//! Everything is `Vec`-ordered — no hash maps — so output order is a pure
//! function of the input.

use ast::{Alignment, Block, BlockKind};

use crate::block::Ctx;
use crate::inline::{self, Atom, Piece};
use crate::{Run, Semantic, StyleId};

/// Rung-2 floor: the narrowest a column may be squeezed.
const COL_FLOOR: u16 = 3;

/// Cell separator; measured through the engine like everything else.
const SEPARATOR: &str = " \u{2502} "; // " │ "

pub(crate) fn layout_table(ctx: &mut Ctx<'_>, alignments: &[Alignment], rows: &[Block]) {
    // Flatten every cell once; remember which rows are header rows.
    let mut grid: Vec<(bool, Vec<Vec<Atom>>)> = Vec::new();
    for row in rows {
        let BlockKind::TableRow { header, cells } = &row.kind else {
            continue;
        };
        let base = if *header {
            Semantic::TableHeader
        } else {
            Semantic::Text
        };
        let cell_atoms: Vec<Vec<Atom>> = cells
            .iter()
            .map(|cell| match &cell.kind {
                BlockKind::TableCell { children } => {
                    // Media inside cells degrades to text (allow_box=false):
                    // a reserved box cannot span a cell region.
                    inline::flatten(children, base, ctx.doc, ctx.sizer, false)
                }
                _ => Vec::new(),
            })
            .collect();
        grid.push((*header, cell_atoms));
    }
    let ncols = alignments
        .len()
        .max(grid.iter().map(|(_, c)| c.len()).max().unwrap_or(0));
    if ncols == 0 || grid.is_empty() {
        return;
    }

    // Intrinsic min-content (widest unbreakable word group) and max-content
    // (widest hard line) width per column.
    let mut min_w = vec![1u16; ncols];
    let mut max_w = vec![1u16; ncols];
    for (_, cells) in &grid {
        for (col, atoms) in cells.iter().enumerate() {
            let (cell_min, cell_max) = intrinsic_widths(atoms, ctx);
            min_w[col] = min_w[col].max(cell_min);
            max_w[col] = max_w[col].max(cell_max);
        }
    }
    for col in 0..ncols {
        max_w[col] = max_w[col].max(min_w[col]);
    }

    let avail = ctx.content_width();
    let sep_w = ctx.engine.display_width(SEPARATOR).min(u16::MAX as usize) as u16;
    // usize throughout: a many-column table can push the separator total past
    // u16::MAX, which would panic (debug) or wrap to a bogus budget (release).
    let sep_total = (sep_w as usize)
        .saturating_mul(ncols.saturating_sub(1))
        .min(u16::MAX as usize) as u16;
    let widths = distribute(&min_w, &max_w, avail.saturating_sub(sep_total));

    // Render: wrap each cell at its column width, pad to width with the
    // column's alignment, join with separators, clip if rung 3 left the
    // composed line over budget.
    let sep_run = Run {
        text: SEPARATOR.to_string(),
        style_id: StyleId::Semantic(Semantic::TableBorder),
        width: sep_w,
        aux: None,
    };
    let header_rows = grid.iter().filter(|(h, _)| *h).count();
    for (ri, (_, cells)) in grid.iter().enumerate() {
        let wrapped: Vec<Vec<Vec<Run>>> = (0..ncols)
            .map(|col| {
                let atoms = cells.get(col).map(|a| collect_atoms(a)).unwrap_or_default();
                let mut lines: Vec<Vec<Run>> = inline::wrap(atoms, widths[col], ctx.engine)
                    .into_iter()
                    .map(|p| match p {
                        Piece::Runs(runs) => runs,
                        Piece::Box(..) => unreachable!("boxes disabled in cells"),
                    })
                    .collect();
                if lines.is_empty() {
                    lines.push(Vec::new());
                }
                lines
            })
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line_i in 0..height {
            let mut runs: Vec<Run> = Vec::new();
            for (col, cell_lines) in wrapped.iter().enumerate() {
                if col > 0 {
                    runs.push(sep_run.clone());
                }
                let content = cell_lines.get(line_i).cloned().unwrap_or_default();
                let align = alignments.get(col).copied().unwrap_or(Alignment::None);
                pad_cell(&mut runs, content, widths[col], align);
            }
            let runs = inline::clip_runs(runs, avail, true, ctx.engine);
            ctx.emit(runs);
        }
        // Rule under the last header row.
        if ri + 1 == header_rows {
            let rule = header_rule(&widths, ctx);
            let rule = inline::clip_runs(rule, avail, true, ctx.engine);
            ctx.emit(rule);
        }
    }
}

/// Re-borrowing helper: `flatten` output is consumed per render, but the
/// grid holds it once — clone atoms into an owned Vec for `wrap`.
fn collect_atoms(atoms: &[Atom]) -> Vec<Atom> {
    atoms
        .iter()
        .map(|a| match a {
            Atom::Word(frags) => Atom::Word(
                frags
                    .iter()
                    .map(|f| inline::Frag {
                        text: f.text.clone(),
                        style: f.style,
                        link: f.link.clone(),
                    })
                    .collect(),
            ),
            Atom::Space => Atom::Space,
            Atom::HardBreak => Atom::HardBreak,
            Atom::Box(id, size) => Atom::Box(*id, *size),
        })
        .collect()
}

/// (min-content, max-content) widths of one cell's atom sequence.
fn intrinsic_widths(atoms: &[Atom], ctx: &Ctx<'_>) -> (u16, u16) {
    let mut min = 0u32;
    let mut max = 0u32;
    let mut line = 0u32;
    let mut pending_space = false;
    for atom in atoms {
        match atom {
            Atom::Word(frags) => {
                let w: u32 = frags
                    .iter()
                    .map(|f| ctx.engine.display_width(&f.text) as u32)
                    .sum();
                min = min.max(w);
                if line > 0 && pending_space {
                    line += 1;
                }
                line += w;
                pending_space = false;
            }
            Atom::Space => pending_space = true,
            Atom::HardBreak => {
                max = max.max(line);
                line = 0;
                pending_space = false;
            }
            Atom::Box(..) => {}
        }
    }
    max = max.max(line);
    (
        min.min(u32::from(u16::MAX)) as u16,
        max.min(u32::from(u16::MAX)) as u16,
    )
}

/// §3.9.3 distribution + ladder rung selection. `avail` is the width left
/// for cell content (separators already subtracted).
fn distribute(min_w: &[u16], max_w: &[u16], avail: u16) -> Vec<u16> {
    let sum = |ws: &[u16]| ws.iter().map(|&w| u32::from(w)).sum::<u32>();
    let avail = u32::from(avail);
    if avail >= sum(max_w) {
        return max_w.to_vec(); // natural width; auto tables don't stretch
    }
    if avail >= sum(min_w) {
        // Rung 1: between min and max — cells wrap at assigned widths.
        return spread(min_w, max_w, avail - sum(min_w));
    }
    let floors: Vec<u16> = min_w.iter().map(|&m| m.clamp(1, COL_FLOOR)).collect();
    if avail >= sum(&floors) {
        // Rung 2: between floor and min — break-anywhere kicks in.
        return spread(&floors, min_w, avail - sum(&floors));
    }
    // Rung 3: even floors don't fit; compose at floors and clip later.
    floors
}

/// Distribute `extra` cells on top of `base`, proportionally to each
/// column's headroom (`target - base`), deterministic left-to-right
/// remainder assignment; never exceeds `target`.
fn spread(base: &[u16], target: &[u16], extra: u32) -> Vec<u16> {
    let headroom: Vec<u32> = base
        .iter()
        .zip(target)
        .map(|(&b, &t)| u32::from(t.saturating_sub(b)))
        .collect();
    let denom: u32 = headroom.iter().sum();
    if denom == 0 {
        return base.to_vec();
    }
    let extra = extra.min(denom);
    let mut widths: Vec<u16> = base
        .iter()
        .zip(&headroom)
        .map(|(&b, &h)| b + (extra * h / denom) as u16)
        .collect();
    let mut leftover = extra
        - widths
            .iter()
            .zip(base)
            .map(|(&w, &b)| u32::from(w - b))
            .sum::<u32>();
    while leftover > 0 {
        let mut gave = false;
        for (w, &t) in widths.iter_mut().zip(target) {
            if leftover == 0 {
                break;
            }
            if *w < t {
                *w += 1;
                leftover -= 1;
                gave = true;
            }
        }
        if !gave {
            break; // all columns at target
        }
    }
    widths
}

/// Pad a wrapped cell line out to `width` cells per its alignment.
fn pad_cell(runs: &mut Vec<Run>, content: Vec<Run>, width: u16, align: Alignment) {
    let content_w: u16 = content.iter().map(|r| r.width).sum();
    let pad = width.saturating_sub(content_w);
    let (left, right) = match align {
        Alignment::None | Alignment::Left => (0, pad),
        Alignment::Right => (pad, 0),
        Alignment::Center => (pad / 2, pad - pad / 2),
    };
    push_pad(runs, left);
    for run in content {
        inline::append(runs, &run.text, run.style_id, run.width);
    }
    push_pad(runs, right);
}

fn push_pad(runs: &mut Vec<Run>, n: u16) {
    if n > 0 {
        inline::append(
            runs,
            &" ".repeat(usize::from(n)),
            StyleId::Semantic(Semantic::Text),
            n,
        );
    }
}

/// The `───┼───` rule under the header, sized to the column widths.
fn header_rule(widths: &[u16], ctx: &Ctx<'_>) -> Vec<Run> {
    let dash = "\u{2500}"; // ─
    let cross = "\u{2500}\u{253C}\u{2500}"; // ─┼─
    let dw = ctx.engine.cluster_width(dash).max(1);
    let mut runs: Vec<Run> = Vec::new();
    let style = StyleId::Semantic(Semantic::TableBorder);
    for (col, &w) in widths.iter().enumerate() {
        if col > 0 {
            let cw = ctx.engine.display_width(cross) as u16;
            inline::append(&mut runs, cross, style, cw);
        }
        let seg = dash.repeat(usize::from(w / dw));
        let sw = ctx.engine.display_width(&seg) as u16;
        inline::append(&mut runs, &seg, style, sw);
    }
    runs
}
