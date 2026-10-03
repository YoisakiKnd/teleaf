//! A small, immutable palette, detected once; rendering never probes the terminal.
use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub incoming: Color,
    pub error: Color,
    pub background: Color,
    pub surface: Color,
    pub selected: Color,
    pub on_accent: Color,
    monochrome: bool,
}

impl Palette {
    pub fn base(self) -> Style {
        Style::default().fg(self.text).bg(self.background)
    }

    pub fn selection(self) -> Style {
        let style = Style::default().fg(self.text).bg(self.selected);
        if self.monochrome {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }

    pub fn primary(self) -> Style {
        let style = Style::default()
            .fg(self.on_accent)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD);
        if self.monochrome {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }
}

const DARK: Palette = Palette {
    name: "深色",
    text: Color::Rgb(225, 233, 239),
    muted: Color::Rgb(150, 169, 181),
    border: Color::Rgb(67, 86, 100),
    accent: Color::Rgb(126, 213, 171),
    incoming: Color::Rgb(151, 191, 225),
    error: Color::Rgb(255, 146, 151),
    background: Color::Rgb(23, 30, 36),
    surface: Color::Rgb(30, 40, 48),
    selected: Color::Rgb(42, 58, 68),
    on_accent: Color::Rgb(15, 42, 30),
    monochrome: false,
};

const LIGHT: Palette = Palette {
    name: "浅色",
    text: Color::Rgb(32, 47, 55),
    muted: Color::Rgb(80, 101, 112),
    border: Color::Rgb(161, 180, 187),
    accent: Color::Rgb(24, 111, 76),
    incoming: Color::Rgb(38, 95, 141),
    error: Color::Rgb(173, 41, 53),
    background: Color::Rgb(246, 249, 247),
    surface: Color::Rgb(233, 240, 235),
    selected: Color::Rgb(216, 232, 222),
    on_accent: Color::Rgb(255, 255, 255),
    monochrome: false,
};

const TERMINAL: Palette = Palette {
    name: "终端配色",
    text: Color::Reset,
    muted: Color::Gray,
    border: Color::DarkGray,
    accent: Color::LightGreen,
    incoming: Color::Cyan,
    error: Color::LightRed,
    background: Color::Reset,
    surface: Color::Reset,
    selected: Color::DarkGray,
    on_accent: Color::Black,
    monochrome: false,
};

fn choose(requested: &str, truecolor: bool, indexed: bool, no_color: bool) -> Palette {
    if no_color {
        return Palette {
            name: "单色",
            text: Color::Reset,
            muted: Color::Reset,
            border: Color::Reset,
            accent: Color::Reset,
            incoming: Color::Reset,
            error: Color::Reset,
            background: Color::Reset,
            surface: Color::Reset,
            selected: Color::Reset,
            on_accent: Color::Reset,
            monochrome: true,
        };
    }
    if requested == "terminal" || (!truecolor && !indexed) {
        return TERMINAL;
    }
    let mut palette = if requested == "light" { LIGHT } else { DARK };
    if !truecolor {
        for color in [
            &mut palette.text,
            &mut palette.muted,
            &mut palette.border,
            &mut palette.accent,
            &mut palette.incoming,
            &mut palette.error,
            &mut palette.background,
            &mut palette.surface,
            &mut palette.selected,
            &mut palette.on_accent,
        ] {
            *color = indexed_color(*color);
        }
    }
    palette
}

// Nearest xterm palette entry, including its grayscale ramp. Only run at startup.
fn indexed_color(color: Color) -> Color {
    let Color::Rgb(r, g, b) = color else {
        return color;
    };
    let levels = [0, 95, 135, 175, 215, 255];
    let mut closest = (u32::MAX, 16);
    for index in 16u16..=255 {
        let (cr, cg, cb) = if index < 232 {
            let n = index - 16;
            (
                levels[(n / 36) as usize],
                levels[(n / 6 % 6) as usize],
                levels[(n % 6) as usize],
            )
        } else {
            let gray = 8 + (index - 232) * 10;
            (gray, gray, gray)
        };
        let distance = (i32::from(r) - i32::from(cr)).pow(2)
            + (i32::from(g) - i32::from(cg)).pow(2)
            + (i32::from(b) - i32::from(cb)).pow(2);
        if (distance as u32) < closest.0 {
            closest = (distance as u32, index as u8);
        }
    }
    Color::Indexed(closest.1)
}

pub fn palette() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(|| {
        let requested = std::env::var("TG_THEME").unwrap_or_else(|_| "auto".into());
        let requested = match requested.as_str() {
            "dark" | "light" | "terminal" => requested.as_str(),
            _ => "auto",
        };
        let term = std::env::var("TERM").unwrap_or_default();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let truecolor = matches!(colorterm.as_str(), "truecolor" | "24bit")
            || matches!(
                term.as_str(),
                "xterm-ghostty" | "xterm-kitty" | "foot" | "foot-extra" | "alacritty"
            )
            || matches!(
                program.as_str(),
                "ghostty" | "iTerm.app" | "WezTerm" | "vscode"
            )
            || std::env::var_os("WT_SESSION").is_some();
        let no_color =
            std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) || term == "dumb";
        choose(requested, truecolor, term.contains("256color"), no_color)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_fallback_preserves_selection_without_color() {
        assert_eq!(choose("auto", false, false, false).background, Color::Reset);
        assert!(matches!(
            choose("auto", false, true, false).accent,
            Color::Indexed(_)
        ));
        assert!(matches!(
            choose("auto", true, false, false).accent,
            Color::Rgb(..)
        ));
        assert_eq!(
            choose("terminal", true, true, false).background,
            Color::Reset
        );
        let mono = choose("dark", true, true, true);
        assert_eq!(mono.background, Color::Reset);
        assert!(mono.selection().add_modifier.contains(Modifier::REVERSED));
        assert!(mono.primary().add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn text_and_actions_have_readable_contrast_in_both_palettes() {
        fn luminance(color: Color) -> f64 {
            let (r, g, b) = match color {
                Color::Rgb(r, g, b) => (r, g, b),
                Color::Indexed(index) if index >= 232 => {
                    let gray = 8 + (index - 232) * 10;
                    (gray, gray, gray)
                }
                Color::Indexed(index) if index >= 16 => {
                    let n = index - 16;
                    let levels = [0, 95, 135, 175, 215, 255];
                    (
                        levels[(n / 36) as usize],
                        levels[(n / 6 % 6) as usize],
                        levels[(n % 6) as usize],
                    )
                }
                _ => panic!("fixed palette required"),
            };
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        }
        let contrast = |a, b| {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        for p in [
            DARK,
            LIGHT,
            choose("dark", false, true, false),
            choose("light", false, true, false),
        ] {
            for fg in [p.text, p.muted, p.accent, p.incoming, p.error] {
                assert!(contrast(fg, p.background) >= 4.5);
                assert!(contrast(fg, p.selected) >= 4.5);
            }
            assert!(contrast(p.on_accent, p.accent) >= 4.5);
        }
    }
}
