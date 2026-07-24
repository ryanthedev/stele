//! Truecolor RGB, a 256-color (xterm cube) downsample, and the color-mode
//! switch that backs `NO_COLOR` support (DW-7.2).

/// A truecolor RGB triple. Mirrors `stele::painter::Color`'s shape exactly
/// (same three `u8` fields) so the `crates/stele/src/decor` bridge is a
/// trivial field-for-field copy — this crate cannot depend on `stele`
/// (`stele::decor` depends on *this* crate instead), so it owns its own
/// copy of the shape rather than sharing a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
}

/// How resolved colors reach the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorMode {
    /// 24-bit truecolor SGR — Ghostty's native mode.
    Truecolor,
    /// Every color is snapped to its nearest entry in the 256-color xterm
    /// cube before use, for terminals without truecolor support.
    Downsample256,
    /// `NO_COLOR` is set: every role resolves with `fg`/`bg` cleared,
    /// keeping only structural attributes (bold/dim/italic/underline).
    NoColor,
}

impl ColorMode {
    /// The `NO_COLOR` convention (<https://no-color.org>): any value,
    /// including an empty string, disables color. Reads the real
    /// environment (the one impure edge here); delegates the actual
    /// decision to [`ColorMode::from_no_color_var`], which a test can call
    /// directly without mutating process-global env state.
    pub fn from_env() -> ColorMode {
        Self::from_no_color_var(std::env::var_os("NO_COLOR"))
    }

    /// Pure decision half of [`ColorMode::from_env`]: presence of *any*
    /// value (including empty) means "no color", absence means truecolor.
    pub fn from_no_color_var(no_color: Option<std::ffi::OsString>) -> ColorMode {
        if no_color.is_some() {
            ColorMode::NoColor
        } else {
            ColorMode::Truecolor
        }
    }
}

/// The 6 intensity steps xterm's 216-color (6×6×6) cube uses per channel.
const CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Snaps `c` to its nearest xterm 256-color-cube entry, returned as the RGB
/// of that entry — not a palette index. [`crate::painter`]-facing `Style`
/// (via the decor bridge) only ever carries truecolor, so a "downsampled"
/// color is still emitted as 24-bit SGR, just quantized to one of the 216
/// values a 256-color terminal can actually distinguish.
pub fn downsample_256(c: Color) -> Color {
    Color::new(nearest_step(c.r), nearest_step(c.g), nearest_step(c.b))
}

fn nearest_step(v: u8) -> u8 {
    CUBE_STEPS
        .iter()
        .copied()
        .min_by_key(|&step| (i16::from(v) - i16::from(step)).unsigned_abs())
        .unwrap_or(0)
}

/// Applies `mode` to `c`. `NoColor` yields `None` (no color at all, not
/// even a quantized one) — the caller must not paint any fg/bg for it.
pub fn apply_mode(c: Color, mode: ColorMode) -> Option<Color> {
    match mode {
        ColorMode::Truecolor => Some(c),
        ColorMode::Downsample256 => Some(downsample_256(c)),
        ColorMode::NoColor => None,
    }
}

/// Converts an HSL triple (`h` in turns `0.0..1.0`, `s`/`l` in `0.0..=1.0`)
/// to RGB. Used by [`crate::theme`] to spread theme-role colors evenly
/// around the color wheel by construction, rather than hand-picking hex
/// values that might accidentally collide once downsampled.
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Color {
    let h = h.rem_euclid(1.0);
    if s <= 0.0 {
        let v = (l * 255.0).round() as u8;
        return Color::new(v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let to_channel = |t: f64| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color::new(
        to_channel(h + 1.0 / 3.0),
        to_channel(h),
        to_channel(h - 1.0 / 3.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_256_snaps_to_nearest_cube_step() {
        assert_eq!(
            downsample_256(Color::new(10, 100, 250)),
            Color::new(0, 95, 255)
        );
        assert_eq!(
            downsample_256(Color::new(255, 255, 255)),
            Color::new(255, 255, 255)
        );
        assert_eq!(downsample_256(Color::new(0, 0, 0)), Color::new(0, 0, 0));
    }

    #[test]
    fn test_apply_mode_no_color_yields_none() {
        let c = Color::new(200, 10, 10);
        assert_eq!(apply_mode(c, ColorMode::NoColor), None);
        assert_eq!(apply_mode(c, ColorMode::Truecolor), Some(c));
        assert!(apply_mode(c, ColorMode::Downsample256).is_some());
    }

    #[test]
    fn test_hsl_to_rgb_primary_hues() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), Color::new(255, 0, 0));
        assert_eq!(hsl_to_rgb(1.0 / 3.0, 1.0, 0.5), Color::new(0, 255, 0));
        assert_eq!(hsl_to_rgb(2.0 / 3.0, 1.0, 0.5), Color::new(0, 0, 255));
    }

    #[test]
    fn test_hsl_to_rgb_zero_saturation_is_gray() {
        let c = hsl_to_rgb(0.3, 0.0, 0.4);
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn test_color_mode_from_no_color_var_respects_presence_not_content() {
        // Presence (even empty) disables color; absence means truecolor.
        // Tested against the pure half directly — mutating real process
        // env vars in a test would race every other `#[test]` in this
        // crate's shared test-binary process, and `set_var`/`remove_var`
        // are `unsafe` (edition 2024), which `forbid(unsafe_code)` bars
        // from this crate outright, tests included.
        assert_eq!(
            ColorMode::from_no_color_var(Some(String::new().into())),
            ColorMode::NoColor
        );
        assert_eq!(
            ColorMode::from_no_color_var(Some("1".into())),
            ColorMode::NoColor
        );
        assert_eq!(ColorMode::from_no_color_var(None), ColorMode::Truecolor);
    }
}
