//! Paletas de cores e temas visuais do ytui.
//!
//! Todos os temas usam cores RGB para manter a identidade visual em qualquer
//! terminal moderno. O tema escolhido é salvo em `~/.config/ytui/config.json`.

use ratatui::style::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Neon,
    Nord,
    Dracula,
    Solarized,
    Mono,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color,
    pub panel: Color,
    pub accent: Color,
    pub accent2: Color,
    pub text: Color,
    pub dim: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub spotify: Color,
}

impl Theme {
    pub const ALL: &'static [Theme] = &[
        Theme::Neon,
        Theme::Nord,
        Theme::Dracula,
        Theme::Solarized,
        Theme::Mono,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Theme::Neon => "neon",
            Theme::Nord => "nord",
            Theme::Dracula => "dracula",
            Theme::Solarized => "solarized",
            Theme::Mono => "mono",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Neon => "Neon",
            Theme::Nord => "Nord",
            Theme::Dracula => "Dracula",
            Theme::Solarized => "Solarized",
            Theme::Mono => "Mono",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "nord" => Theme::Nord,
            "dracula" => Theme::Dracula,
            "solarized" => Theme::Solarized,
            "mono" | "classico" | "clássico" => Theme::Mono,
            _ => Theme::Neon,
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Theme::Neon => Palette {
                bg: Color::Rgb(8, 12, 18),
                panel: Color::Rgb(15, 22, 32),
                accent: Color::Rgb(34, 211, 238),
                accent2: Color::Rgb(167, 139, 250),
                text: Color::Rgb(226, 232, 240),
                dim: Color::Rgb(100, 116, 139),
                success: Color::Rgb(52, 211, 153),
                warning: Color::Rgb(251, 191, 36),
                error: Color::Rgb(248, 113, 113),
                border: Color::Rgb(30, 41, 59),
                selected_bg: Color::Rgb(14, 116, 144),
                selected_fg: Color::Rgb(236, 254, 255),
                spotify: Color::Rgb(29, 185, 84),
            },
            Theme::Nord => Palette {
                bg: Color::Rgb(46, 52, 64),
                panel: Color::Rgb(59, 66, 82),
                accent: Color::Rgb(136, 192, 208),
                accent2: Color::Rgb(180, 142, 173),
                text: Color::Rgb(236, 239, 244),
                dim: Color::Rgb(123, 136, 161),
                success: Color::Rgb(163, 190, 140),
                warning: Color::Rgb(235, 203, 139),
                error: Color::Rgb(191, 97, 106),
                border: Color::Rgb(76, 86, 106),
                selected_bg: Color::Rgb(67, 76, 94),
                selected_fg: Color::Rgb(236, 239, 244),
                spotify: Color::Rgb(29, 185, 84),
            },
            Theme::Dracula => Palette {
                bg: Color::Rgb(40, 42, 54),
                panel: Color::Rgb(49, 51, 66),
                accent: Color::Rgb(189, 147, 249),
                accent2: Color::Rgb(255, 121, 198),
                text: Color::Rgb(248, 248, 242),
                dim: Color::Rgb(98, 114, 164),
                success: Color::Rgb(80, 250, 123),
                warning: Color::Rgb(241, 250, 140),
                error: Color::Rgb(255, 85, 85),
                border: Color::Rgb(68, 71, 90),
                selected_bg: Color::Rgb(68, 71, 90),
                selected_fg: Color::Rgb(248, 248, 242),
                spotify: Color::Rgb(29, 185, 84),
            },
            Theme::Solarized => Palette {
                bg: Color::Rgb(0, 43, 54),
                panel: Color::Rgb(7, 54, 66),
                accent: Color::Rgb(42, 161, 152),
                accent2: Color::Rgb(38, 139, 210),
                text: Color::Rgb(238, 232, 213),
                dim: Color::Rgb(131, 148, 150),
                success: Color::Rgb(133, 153, 0),
                warning: Color::Rgb(181, 137, 0),
                error: Color::Rgb(220, 50, 47),
                border: Color::Rgb(88, 110, 117),
                selected_bg: Color::Rgb(11, 74, 90),
                selected_fg: Color::Rgb(253, 246, 227),
                spotify: Color::Rgb(29, 185, 84),
            },
            Theme::Mono => Palette {
                bg: Color::Rgb(0, 0, 0),
                panel: Color::Rgb(16, 16, 16),
                accent: Color::Rgb(255, 255, 255),
                accent2: Color::Rgb(170, 170, 170),
                text: Color::Rgb(238, 238, 238),
                dim: Color::Rgb(119, 119, 119),
                success: Color::Rgb(187, 187, 187),
                warning: Color::Rgb(221, 221, 221),
                error: Color::Rgb(255, 255, 255),
                border: Color::Rgb(51, 51, 51),
                selected_bg: Color::Rgb(255, 255, 255),
                selected_fg: Color::Rgb(0, 0, 0),
                spotify: Color::Rgb(221, 221, 221),
            },
        }
    }
}
