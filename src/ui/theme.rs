use eframe::egui;

#[derive(Clone)]
pub struct ThemeColors {
    pub bg_primary: egui::Color32,
    pub bg_secondary: egui::Color32,
    pub bg_elevated: egui::Color32,
    pub fg_primary: egui::Color32,
    pub fg_dim: egui::Color32,
    pub accent: egui::Color32,
    pub green: egui::Color32,
    pub red: egui::Color32,
    pub cursor_color: egui::Color32,
    pub hover_bg: egui::Color32,
    pub hover_shadow: egui::Color32,
    pub border: egui::Color32,

    // Semantic colors derived from base colors
    pub card_bg: egui::Color32,         // List item / card background
    pub card_hover: egui::Color32,      // List item hover
    pub input_bg: egui::Color32,        // Text input background
    pub input_border: egui::Color32,    // Text input border
    pub badge_bg: egui::Color32,        // Tag / badge background
    pub focus_ring: egui::Color32,      // Focus indicator
    pub divider: egui::Color32,         // Section divider line
    pub overlay_bg: egui::Color32,      // Modal overlay background
    pub success_dim: egui::Color32,     // Dimmed success (for backgrounds)
    pub error_dim: egui::Color32,       // Dimmed error (for backgrounds)
}

impl ThemeColors {
    pub fn accent_alpha(&self, alpha: u8) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(self.accent.r(), self.accent.g(), self.accent.b(), alpha)
    }

    /// A very dim color for input placeholder/hint text, clearly distinguishable from real input.
    pub fn hint_color(&self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(
            self.fg_dim.r(),
            self.fg_dim.g(),
            self.fg_dim.b(),
            80,
        )
    }
}

pub fn darker(c: egui::Color32, amount: u8) -> egui::Color32 {
    egui::Color32::from_rgb(
        c.r().saturating_sub(amount),
        c.g().saturating_sub(amount),
        c.b().saturating_sub(amount),
    )
}

pub fn brighter(c: egui::Color32, amount: u8) -> egui::Color32 {
    egui::Color32::from_rgb(
        c.r().saturating_add(amount),
        c.g().saturating_add(amount),
        c.b().saturating_add(amount),
    )
}

#[derive(Clone, Copy, PartialEq)]
pub enum ThemePreset {
    TokyoNight,
    Dracula,
    OneDark,
    SolarizedDark,
    Nord,
}

impl ThemePreset {
    pub fn colors(&self) -> ThemeColors {
        match self {
            ThemePreset::TokyoNight => ThemeColors {
                bg_primary: egui::Color32::from_rgb(26, 27, 38),
                bg_secondary: egui::Color32::from_rgb(30, 31, 46),
                bg_elevated: egui::Color32::from_rgb(36, 40, 59),
                fg_primary: egui::Color32::from_rgb(220, 228, 255),
                fg_dim: egui::Color32::from_rgb(145, 155, 185),
                accent: egui::Color32::from_rgb(122, 162, 247),
                green: egui::Color32::from_rgb(115, 218, 202),
                red: egui::Color32::from_rgb(247, 118, 142),
                cursor_color: egui::Color32::from_rgb(220, 228, 255),
                hover_bg: egui::Color32::from_rgb(38, 40, 56),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                border: egui::Color32::from_rgb(40, 42, 58),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(34, 35, 46),           // brighter(bg_primary, 8)
                card_hover: egui::Color32::from_rgb(40, 42, 60),
                input_bg: egui::Color32::from_rgb(20, 21, 36),          // darker(bg_secondary, 10)
                input_border: egui::Color32::from_rgb(56, 60, 79),      // brighter(bg_elevated, 20)
                badge_bg: egui::Color32::from_rgba_unmultiplied(122, 162, 247, 20),
                focus_ring: egui::Color32::from_rgba_unmultiplied(122, 162, 247, 120),
                divider: egui::Color32::from_rgb(45, 47, 65),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(26, 27, 38, 220),
                success_dim: egui::Color32::from_rgba_unmultiplied(115, 218, 202, 30),
                error_dim: egui::Color32::from_rgba_unmultiplied(247, 118, 142, 30),
            },
            ThemePreset::Dracula => ThemeColors {
                bg_primary: egui::Color32::from_rgb(40, 42, 54),
                bg_secondary: egui::Color32::from_rgb(44, 44, 68),
                bg_elevated: egui::Color32::from_rgb(55, 56, 77),
                fg_primary: egui::Color32::from_rgb(248, 248, 242),
                fg_dim: egui::Color32::from_rgb(176, 176, 168),
                accent: egui::Color32::from_rgb(189, 147, 249),
                green: egui::Color32::from_rgb(80, 250, 123),
                red: egui::Color32::from_rgb(255, 85, 85),
                cursor_color: egui::Color32::from_rgb(248, 248, 242),
                hover_bg: egui::Color32::from_rgb(52, 54, 72),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                border: egui::Color32::from_rgb(58, 60, 80),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(48, 50, 62),           // brighter(bg_primary, 8)
                card_hover: egui::Color32::from_rgb(56, 58, 76),
                input_bg: egui::Color32::from_rgb(34, 34, 58),          // darker(bg_secondary, 10)
                input_border: egui::Color32::from_rgb(75, 76, 97),      // brighter(bg_elevated, 20)
                badge_bg: egui::Color32::from_rgba_unmultiplied(189, 147, 249, 20),
                focus_ring: egui::Color32::from_rgba_unmultiplied(189, 147, 249, 120),
                divider: egui::Color32::from_rgb(62, 64, 85),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(40, 42, 54, 220),
                success_dim: egui::Color32::from_rgba_unmultiplied(80, 250, 123, 30),
                error_dim: egui::Color32::from_rgba_unmultiplied(255, 85, 85, 30),
            },
            ThemePreset::OneDark => ThemeColors {
                bg_primary: egui::Color32::from_rgb(40, 44, 52),
                bg_secondary: egui::Color32::from_rgb(33, 37, 43),
                bg_elevated: egui::Color32::from_rgb(50, 55, 65),
                fg_primary: egui::Color32::from_rgb(220, 223, 230),
                fg_dim: egui::Color32::from_rgb(145, 152, 165),
                accent: egui::Color32::from_rgb(97, 175, 239),
                green: egui::Color32::from_rgb(152, 195, 121),
                red: egui::Color32::from_rgb(224, 108, 117),
                cursor_color: egui::Color32::from_rgb(220, 223, 230),
                hover_bg: egui::Color32::from_rgb(50, 55, 65),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                border: egui::Color32::from_rgb(53, 58, 68),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(48, 52, 60),           // brighter(bg_primary, 8)
                card_hover: egui::Color32::from_rgb(55, 60, 70),
                input_bg: egui::Color32::from_rgb(23, 27, 33),          // darker(bg_secondary, 10)
                input_border: egui::Color32::from_rgb(70, 75, 85),      // brighter(bg_elevated, 20)
                badge_bg: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 20),
                focus_ring: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 120),
                divider: egui::Color32::from_rgb(58, 63, 73),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(40, 44, 52, 220),
                success_dim: egui::Color32::from_rgba_unmultiplied(152, 195, 121, 30),
                error_dim: egui::Color32::from_rgba_unmultiplied(224, 108, 117, 30),
            },
            ThemePreset::SolarizedDark => ThemeColors {
                bg_primary: egui::Color32::from_rgb(0, 43, 54),
                bg_secondary: egui::Color32::from_rgb(7, 54, 66),
                bg_elevated: egui::Color32::from_rgb(14, 65, 78),
                fg_primary: egui::Color32::from_rgb(213, 225, 227),
                fg_dim: egui::Color32::from_rgb(147, 161, 161),
                accent: egui::Color32::from_rgb(38, 139, 210),
                green: egui::Color32::from_rgb(133, 153, 0),
                red: egui::Color32::from_rgb(220, 50, 47),
                cursor_color: egui::Color32::from_rgb(213, 225, 227),
                hover_bg: egui::Color32::from_rgb(7, 54, 66),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                border: egui::Color32::from_rgb(20, 72, 85),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(8, 51, 62),            // brighter(bg_primary, 8)
                card_hover: egui::Color32::from_rgb(14, 60, 72),
                input_bg: egui::Color32::from_rgb(0, 44, 56),           // darker(bg_secondary, 10)
                input_border: egui::Color32::from_rgb(34, 85, 98),      // brighter(bg_elevated, 20)
                badge_bg: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 20),
                focus_ring: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 120),
                divider: egui::Color32::from_rgb(18, 68, 80),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(0, 43, 54, 220),
                success_dim: egui::Color32::from_rgba_unmultiplied(133, 153, 0, 30),
                error_dim: egui::Color32::from_rgba_unmultiplied(220, 50, 47, 30),
            },
            ThemePreset::Nord => ThemeColors {
                bg_primary: egui::Color32::from_rgb(46, 52, 64),
                bg_secondary: egui::Color32::from_rgb(59, 66, 82),
                bg_elevated: egui::Color32::from_rgb(67, 76, 94),
                fg_primary: egui::Color32::from_rgb(236, 239, 244),
                fg_dim: egui::Color32::from_rgb(165, 177, 194),
                accent: egui::Color32::from_rgb(136, 192, 208),
                green: egui::Color32::from_rgb(163, 190, 140),
                red: egui::Color32::from_rgb(191, 97, 106),
                cursor_color: egui::Color32::from_rgb(236, 239, 244),
                hover_bg: egui::Color32::from_rgb(59, 66, 82),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                border: egui::Color32::from_rgb(72, 80, 100),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(54, 60, 72),           // brighter(bg_primary, 8)
                card_hover: egui::Color32::from_rgb(63, 70, 86),
                input_bg: egui::Color32::from_rgb(49, 56, 72),          // darker(bg_secondary, 10)
                input_border: egui::Color32::from_rgb(87, 96, 114),     // brighter(bg_elevated, 20)
                badge_bg: egui::Color32::from_rgba_unmultiplied(136, 192, 208, 20),
                focus_ring: egui::Color32::from_rgba_unmultiplied(136, 192, 208, 120),
                divider: egui::Color32::from_rgb(76, 84, 100),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(46, 52, 64, 220),
                success_dim: egui::Color32::from_rgba_unmultiplied(163, 190, 140, 30),
                error_dim: egui::Color32::from_rgba_unmultiplied(191, 97, 106, 30),
            },
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ThemePreset::TokyoNight => "Tokyo Night",
            ThemePreset::Dracula => "Dracula",
            ThemePreset::OneDark => "One Dark",
            ThemePreset::SolarizedDark => "Solarized Dark",
            ThemePreset::Nord => "Nord",
        }
    }

    #[allow(dead_code)]
    pub fn id(&self) -> &'static str {
        match self {
            ThemePreset::TokyoNight => "tokyo_night",
            ThemePreset::Dracula => "dracula",
            ThemePreset::OneDark => "one_dark",
            ThemePreset::SolarizedDark => "solarized_dark",
            ThemePreset::Nord => "nord",
        }
    }

    pub fn all() -> &'static [ThemePreset] {
        &[
            ThemePreset::TokyoNight,
            ThemePreset::Dracula,
            ThemePreset::OneDark,
            ThemePreset::SolarizedDark,
            ThemePreset::Nord,
        ]
    }

    #[allow(dead_code)]
    pub fn from_id(id: &str) -> Self {
        match id {
            "dracula" => ThemePreset::Dracula,
            "one_dark" => ThemePreset::OneDark,
            "solarized_dark" => ThemePreset::SolarizedDark,
            "nord" => ThemePreset::Nord,
            _ => ThemePreset::TokyoNight,
        }
    }
}
