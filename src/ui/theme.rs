//! Theme engine with TrueColor/256-color adaptation.

use ratatui::style::Color;

/// Built-in themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeName {
    #[default]
    TokyoNight,
    CatppuccinMocha,
    GruvboxDark,
    OneDarkPro,
    CatppuccinLatte,
}

impl ThemeName {
    pub const ALL: [ThemeName; 5] = [
        ThemeName::TokyoNight,
        ThemeName::CatppuccinMocha,
        ThemeName::GruvboxDark,
        ThemeName::OneDarkPro,
        ThemeName::CatppuccinLatte,
    ];

    pub fn parse(value: &str) -> Self {
        let normalized = value
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_', ' '], "");
        match normalized.as_str() {
            "catppuccinmocha" | "catppuccin" | "mocha" => Self::CatppuccinMocha,
            "gruvboxdark" | "gruvbox" => Self::GruvboxDark,
            "onedarkpro" | "onedark" => Self::OneDarkPro,
            "catppuccinlatte" | "latte" => Self::CatppuccinLatte,
            _ => Self::TokyoNight,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TokyoNight => "TokyoNight",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::OneDarkPro => "One Dark Pro",
            Self::CatppuccinLatte => "Catppuccin Latte",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::TokyoNight => "tokyonight",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::GruvboxDark => "gruvbox-dark",
            Self::OneDarkPro => "one-dark-pro",
            Self::CatppuccinLatte => "catppuccin-latte",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|name| *name == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn is_light(self) -> bool {
        matches!(self, Self::CatppuccinLatte)
    }
}

/// Detected terminal color depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorDepth {
    #[default]
    Ansi256,
    TrueColor,
}

impl ColorDepth {
    /// Detect from `COLORTERM`, unless the user explicitly configured it.
    pub fn detect(override_truecolor: Option<bool>) -> Self {
        match override_truecolor {
            Some(true) => Self::TrueColor,
            Some(false) => Self::Ansi256,
            None => match std::env::var("COLORTERM") {
                Ok(value)
                    if value.eq_ignore_ascii_case("truecolor")
                        || value.eq_ignore_ascii_case("24bit") =>
                {
                    Self::TrueColor
                }
                _ => Self::Ansi256,
            },
        }
    }
}

/// Semantic UI colors.
#[derive(Debug, Clone, Copy)]
pub struct UiTheme {
    pub background: Color,
    pub foreground: Color,
    pub dim: Color,
    pub border: Color,
    pub border_focused: Color,
    pub accent: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub selection: Color,
    pub cursor: Color,
    pub cursor_fg: Color,
    pub panel_bg: Color,
    pub tab_bar_bg: Color,
    pub tab_active: Color,
    pub tab_inactive: Color,
    pub error: Color,
    pub warning: Color,
    pub hint: Color,
    pub success: Color,
}

/// Syntax token colors.
#[derive(Debug, Clone, Copy)]
pub struct SyntaxTheme {
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    pub type_name: Color,
    pub function: Color,
    pub constant: Color,
    pub operator: Color,
}

/// Complete theme.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: ThemeName,
    pub depth: ColorDepth,
    pub ui: UiTheme,
    pub syntax: SyntaxTheme,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_name(ThemeName::TokyoNight, ColorDepth::default())
    }
}

impl Theme {
    pub fn from_name(name: ThemeName, depth: ColorDepth) -> Self {
        let (ui, syntax) = match name {
            ThemeName::TokyoNight => (
                UiTheme {
                    background: rgb(depth, 0x1a, 0x1b, 0x26),
                    foreground: rgb(depth, 0xc0, 0xca, 0xf5),
                    dim: rgb(depth, 0x56, 0x5f, 0x89),
                    border: rgb(depth, 0x3b, 0x42, 0x61),
                    border_focused: rgb(depth, 0x7a, 0xa2, 0xf7),
                    accent: rgb(depth, 0x7a, 0xa2, 0xf7),
                    status_bg: rgb(depth, 0x16, 0x16, 0x1e),
                    status_fg: rgb(depth, 0xc0, 0xca, 0xf5),
                    selection: rgb(depth, 0x28, 0x34, 0x57),
                    cursor: rgb(depth, 0xc0, 0xca, 0xf5),
                    cursor_fg: rgb(depth, 0x1a, 0x1b, 0x26),
                    panel_bg: rgb(depth, 0x1a, 0x1b, 0x26),
                    tab_bar_bg: rgb(depth, 0x14, 0x14, 0x1e),
                    tab_active: rgb(depth, 0x7a, 0xa2, 0xf7),
                    tab_inactive: rgb(depth, 0x56, 0x5f, 0x89),
                    error: rgb(depth, 0xf7, 0x76, 0x8e),
                    warning: rgb(depth, 0xe0, 0xaf, 0x68),
                    hint: rgb(depth, 0x7d, 0xcf, 0xff),
                    success: rgb(depth, 0x9e, 0xce, 0x6a),
                },
                SyntaxTheme {
                    keyword: rgb(depth, 0xbb, 0x9a, 0xf7),
                    string: rgb(depth, 0x9e, 0xce, 0x6a),
                    comment: rgb(depth, 0x56, 0x5f, 0x89),
                    number: rgb(depth, 0xff, 0x9e, 0x64),
                    type_name: rgb(depth, 0x2a, 0xc3, 0xde),
                    function: rgb(depth, 0x7a, 0xa2, 0xf7),
                    constant: rgb(depth, 0xff, 0x9e, 0x64),
                    operator: rgb(depth, 0x89, 0xdd, 0xff),
                },
            ),
            ThemeName::CatppuccinMocha => (
                UiTheme {
                    background: rgb(depth, 0x1e, 0x1e, 0x2e),
                    foreground: rgb(depth, 0xcd, 0xd6, 0xf4),
                    dim: rgb(depth, 0x6c, 0x70, 0x86),
                    border: rgb(depth, 0x45, 0x47, 0x5a),
                    border_focused: rgb(depth, 0x89, 0xb4, 0xfa),
                    accent: rgb(depth, 0x89, 0xb4, 0xfa),
                    status_bg: rgb(depth, 0x18, 0x18, 0x25),
                    status_fg: rgb(depth, 0xcd, 0xd6, 0xf4),
                    selection: rgb(depth, 0x31, 0x32, 0x44),
                    cursor: rgb(depth, 0xf5, 0xe0, 0xdc),
                    cursor_fg: rgb(depth, 0x1e, 0x1e, 0x2e),
                    panel_bg: rgb(depth, 0x1e, 0x1e, 0x2e),
                    tab_bar_bg: rgb(depth, 0x17, 0x17, 0x26),
                    tab_active: rgb(depth, 0x89, 0xb4, 0xfa),
                    tab_inactive: rgb(depth, 0x6c, 0x70, 0x86),
                    error: rgb(depth, 0xf3, 0x8b, 0xa8),
                    warning: rgb(depth, 0xf9, 0xe2, 0xaf),
                    hint: rgb(depth, 0x89, 0xd8, 0xeb),
                    success: rgb(depth, 0xa6, 0xe3, 0xa1),
                },
                SyntaxTheme {
                    keyword: rgb(depth, 0xcb, 0xa6, 0xf7),
                    string: rgb(depth, 0xa6, 0xe3, 0xa1),
                    comment: rgb(depth, 0x6c, 0x70, 0x86),
                    number: rgb(depth, 0xfa, 0xb3, 0x87),
                    type_name: rgb(depth, 0x94, 0xe2, 0xd5),
                    function: rgb(depth, 0x89, 0xb4, 0xfa),
                    constant: rgb(depth, 0xfa, 0xb3, 0x87),
                    operator: rgb(depth, 0x89, 0xd8, 0xeb),
                },
            ),
            ThemeName::GruvboxDark => (
                UiTheme {
                    background: rgb(depth, 0x28, 0x28, 0x28),
                    foreground: rgb(depth, 0xeb, 0xdb, 0xb2),
                    dim: rgb(depth, 0x92, 0x83, 0x74),
                    border: rgb(depth, 0x50, 0x49, 0x45),
                    border_focused: rgb(depth, 0xfa, 0xbd, 0x2f),
                    accent: rgb(depth, 0xfa, 0xbd, 0x2f),
                    status_bg: rgb(depth, 0x1d, 0x20, 0x21),
                    status_fg: rgb(depth, 0xeb, 0xdb, 0xb2),
                    selection: rgb(depth, 0x3c, 0x38, 0x36),
                    cursor: rgb(depth, 0xeb, 0xdb, 0xb2),
                    cursor_fg: rgb(depth, 0x28, 0x28, 0x28),
                    panel_bg: rgb(depth, 0x28, 0x28, 0x28),
                    tab_bar_bg: rgb(depth, 0x1d, 0x1d, 0x1d),
                    tab_active: rgb(depth, 0xfa, 0xbd, 0x2f),
                    tab_inactive: rgb(depth, 0x92, 0x83, 0x74),
                    error: rgb(depth, 0xfb, 0x49, 0x34),
                    warning: rgb(depth, 0xfa, 0xbd, 0x2f),
                    hint: rgb(depth, 0x8e, 0xc0, 0x7c),
                    success: rgb(depth, 0xb8, 0xbb, 0x26),
                },
                SyntaxTheme {
                    keyword: rgb(depth, 0xd3, 0x86, 0x9b),
                    string: rgb(depth, 0xb8, 0xbb, 0x26),
                    comment: rgb(depth, 0x92, 0x83, 0x74),
                    number: rgb(depth, 0xfe, 0x80, 0x19),
                    type_name: rgb(depth, 0x83, 0xa5, 0x98),
                    function: rgb(depth, 0xfa, 0xbd, 0x2f),
                    constant: rgb(depth, 0xfe, 0x80, 0x19),
                    operator: rgb(depth, 0x8e, 0xc0, 0x7c),
                },
            ),
            ThemeName::OneDarkPro => (
                UiTheme {
                    background: rgb(depth, 0x28, 0x2c, 0x34),
                    foreground: rgb(depth, 0xab, 0xb2, 0xbf),
                    dim: rgb(depth, 0x5c, 0x63, 0x70),
                    border: rgb(depth, 0x3e, 0x44, 0x51),
                    border_focused: rgb(depth, 0x61, 0xaf, 0xef),
                    accent: rgb(depth, 0x61, 0xaf, 0xef),
                    status_bg: rgb(depth, 0x21, 0x25, 0x2b),
                    status_fg: rgb(depth, 0xab, 0xb2, 0xbf),
                    selection: rgb(depth, 0x3e, 0x44, 0x51),
                    cursor: rgb(depth, 0x52, 0x8b, 0xff),
                    cursor_fg: rgb(depth, 0x28, 0x2c, 0x34),
                    panel_bg: rgb(depth, 0x28, 0x2c, 0x34),
                    tab_bar_bg: rgb(depth, 0x20, 0x23, 0x2b),
                    tab_active: rgb(depth, 0x61, 0xaf, 0xef),
                    tab_inactive: rgb(depth, 0x5c, 0x63, 0x70),
                    error: rgb(depth, 0xe0, 0x6c, 0x75),
                    warning: rgb(depth, 0xe5, 0xc0, 0x7b),
                    hint: rgb(depth, 0x56, 0xb6, 0xc2),
                    success: rgb(depth, 0x98, 0xc3, 0x79),
                },
                SyntaxTheme {
                    keyword: rgb(depth, 0xc6, 0x78, 0xdd),
                    string: rgb(depth, 0x98, 0xc3, 0x79),
                    comment: rgb(depth, 0x5c, 0x63, 0x70),
                    number: rgb(depth, 0xd1, 0x9a, 0x66),
                    type_name: rgb(depth, 0xe5, 0xc0, 0x7b),
                    function: rgb(depth, 0x61, 0xaf, 0xef),
                    constant: rgb(depth, 0xd1, 0x9a, 0x66),
                    operator: rgb(depth, 0x56, 0xb6, 0xc2),
                },
            ),
            ThemeName::CatppuccinLatte => (
                UiTheme {
                    background: rgb(depth, 0xef, 0xf1, 0xf5),
                    foreground: rgb(depth, 0x4c, 0x4f, 0x69),
                    dim: rgb(depth, 0x8c, 0x8f, 0xa1),
                    border: rgb(depth, 0xbc, 0xc0, 0xcc),
                    border_focused: rgb(depth, 0x1e, 0x66, 0xf5),
                    accent: rgb(depth, 0x1e, 0x66, 0xf5),
                    status_bg: rgb(depth, 0xe6, 0xe9, 0xef),
                    status_fg: rgb(depth, 0x4c, 0x4f, 0x69),
                    selection: rgb(depth, 0xcc, 0xd0, 0xda),
                    cursor: rgb(depth, 0xdc, 0x8a, 0x78),
                    cursor_fg: rgb(depth, 0xef, 0xf1, 0xf5),
                    panel_bg: rgb(depth, 0xef, 0xf1, 0xf5),
                    tab_bar_bg: rgb(depth, 0xe2, 0xe5, 0xeb),
                    tab_active: rgb(depth, 0x1e, 0x66, 0xf5),
                    tab_inactive: rgb(depth, 0x8c, 0x8f, 0xa1),
                    error: rgb(depth, 0xd2, 0x0f, 0x39),
                    warning: rgb(depth, 0xdf, 0x8e, 0x1d),
                    hint: rgb(depth, 0x17, 0x92, 0x99),
                    success: rgb(depth, 0x40, 0xa0, 0x2b),
                },
                SyntaxTheme {
                    keyword: rgb(depth, 0x88, 0x39, 0xef),
                    string: rgb(depth, 0x40, 0xa0, 0x2b),
                    comment: rgb(depth, 0x8c, 0x8f, 0xa1),
                    number: rgb(depth, 0xfe, 0x64, 0x0b),
                    type_name: rgb(depth, 0x17, 0x92, 0x99),
                    function: rgb(depth, 0x1e, 0x66, 0xf5),
                    constant: rgb(depth, 0xfe, 0x64, 0x0b),
                    operator: rgb(depth, 0x04, 0xa5, 0xe5),
                },
            ),
        };
        Self {
            name,
            depth,
            ui,
            syntax,
        }
    }
}

fn rgb(depth: ColorDepth, r: u8, g: u8, b: u8) -> Color {
    match depth {
        ColorDepth::TrueColor => Color::Rgb(r, g, b),
        ColorDepth::Ansi256 => Color::Indexed(quantize_256(r, g, b)),
    }
}

/// Map an RGB triple to the nearest xterm-256 cube index.
pub fn quantize_256(r: u8, g: u8, b: u8) -> u8 {
    fn level(value: u8) -> u8 {
        const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let mut best = 0;
        let mut best_distance = u16::MAX;
        for (index, step) in STEPS.iter().enumerate() {
            let distance = (i32::from(value) - i32::from(*step)).unsigned_abs() as u16;
            if distance < best_distance {
                best_distance = distance;
                best = index as u8;
            }
        }
        best
    }
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_parse_aliases() {
        assert_eq!(ThemeName::parse("tokyo-night"), ThemeName::TokyoNight);
        assert_eq!(
            ThemeName::parse("catppuccin mocha"),
            ThemeName::CatppuccinMocha
        );
        assert_eq!(ThemeName::parse("unknown"), ThemeName::TokyoNight);
    }

    #[test]
    fn quantizer_produces_valid_cube_indices() {
        assert_eq!(quantize_256(0, 0, 0), 16);
        assert_eq!(quantize_256(255, 255, 255), 231);
        let index = quantize_256(120, 200, 90);
        assert!((16..=231).contains(&index));
    }

    #[test]
    fn ansi256_theme_uses_indexed_colors() {
        let theme = Theme::from_name(ThemeName::TokyoNight, ColorDepth::Ansi256);
        assert!(matches!(theme.ui.background, Color::Indexed(_)));
    }

    #[test]
    fn next_cycles_through_all_themes() {
        let mut name = ThemeName::TokyoNight;
        for _ in 0..ThemeName::ALL.len() {
            name = name.next();
        }
        assert_eq!(name, ThemeName::TokyoNight);
    }
}
