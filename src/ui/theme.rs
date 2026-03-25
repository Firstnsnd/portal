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
    #[allow(dead_code)]
    pub card_bg: egui::Color32,         // List item / card background
    #[allow(dead_code)]
    pub card_hover: egui::Color32,      // List item hover
    pub input_bg: egui::Color32,        // Text input background
    pub input_border: egui::Color32,    // Text input border
    pub button_bg: egui::Color32,       // Button background
    pub badge_bg: egui::Color32,        // Tag / badge background
    pub menu_bg: egui::Color32,         // Dropdown menu / popup background
    pub focus_ring: egui::Color32,      // Focus indicator
    #[allow(dead_code)]
    pub divider: egui::Color32,         // Section divider line
    #[allow(dead_code)]
    pub overlay_bg: egui::Color32,      // Modal overlay background
    #[allow(dead_code)]
    pub success_dim: egui::Color32,     // Dimmed success (for backgrounds)
    #[allow(dead_code)]
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

#[derive(Clone, Copy, PartialEq)]
pub enum ThemePreset {
    TokyoNight,
    Dracula,
    OneDark,
    SolarizedDark,
    Nord,
    SolarizedLight,
    GitHubLight,
    OneLight,
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
                card_bg: egui::Color32::from_rgb(34, 35, 46),
                card_hover: egui::Color32::from_rgb(40, 42, 60),
                input_bg: egui::Color32::from_rgb(20, 21, 36),
                input_border: egui::Color32::from_rgb(56, 60, 79),
                button_bg: egui::Color32::from_rgb(45, 50, 70),         // Darker for light text
                badge_bg: egui::Color32::from_rgba_unmultiplied(122, 162, 247, 20),
                menu_bg: egui::Color32::from_rgb(32, 34, 48),          // Dropdown menu background
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
                card_bg: egui::Color32::from_rgb(48, 50, 62),
                card_hover: egui::Color32::from_rgb(56, 58, 76),
                input_bg: egui::Color32::from_rgb(34, 34, 58),
                input_border: egui::Color32::from_rgb(75, 76, 97),
                button_bg: egui::Color32::from_rgb(60, 65, 85),         // Darker for light text
                badge_bg: egui::Color32::from_rgba_unmultiplied(189, 147, 249, 20),
                menu_bg: egui::Color32::from_rgb(48, 50, 68),          // Dropdown menu background
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
                card_bg: egui::Color32::from_rgb(48, 52, 60),
                card_hover: egui::Color32::from_rgb(55, 60, 70),
                input_bg: egui::Color32::from_rgb(23, 27, 33),
                input_border: egui::Color32::from_rgb(70, 75, 85),
                button_bg: egui::Color32::from_rgb(60, 70, 85),         // Darker for light text
                badge_bg: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 20),
                menu_bg: egui::Color32::from_rgb(46, 50, 60),          // Dropdown menu background
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
                card_bg: egui::Color32::from_rgb(8, 51, 62),
                card_hover: egui::Color32::from_rgb(14, 60, 72),
                input_bg: egui::Color32::from_rgb(0, 44, 56),
                input_border: egui::Color32::from_rgb(34, 85, 98),
                button_bg: egui::Color32::from_rgb(18, 80, 95),         // Darker for light text
                badge_bg: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 20),
                menu_bg: egui::Color32::from_rgb(10, 58, 70),          // Dropdown menu background
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
                card_bg: egui::Color32::from_rgb(54, 60, 72),
                card_hover: egui::Color32::from_rgb(63, 70, 86),
                input_bg: egui::Color32::from_rgb(49, 56, 72),
                input_border: egui::Color32::from_rgb(87, 96, 114),
                button_bg: egui::Color32::from_rgb(75, 90, 105),        // Darker for light text
                badge_bg: egui::Color32::from_rgba_unmultiplied(136, 192, 208, 20),
                menu_bg: egui::Color32::from_rgb(54, 62, 76),          // Dropdown menu background
                focus_ring: egui::Color32::from_rgba_unmultiplied(136, 192, 208, 120),
                divider: egui::Color32::from_rgb(76, 84, 100),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(46, 52, 64, 220),
                success_dim: egui::Color32::from_rgba_unmultiplied(163, 190, 140, 30),
                error_dim: egui::Color32::from_rgba_unmultiplied(191, 97, 106, 30),
            },
            ThemePreset::SolarizedLight => ThemeColors {
                bg_primary: egui::Color32::from_rgb(253, 246, 227),
                bg_secondary: egui::Color32::from_rgb(245, 235, 205),
                bg_elevated: egui::Color32::from_rgb(238, 232, 213),
                fg_primary: egui::Color32::from_rgb(88, 110, 117),         // Dark text
                fg_dim: egui::Color32::from_rgb(131, 148, 150),
                accent: egui::Color32::from_rgb(38, 139, 210),
                green: egui::Color32::from_rgb(133, 153, 0),
                red: egui::Color32::from_rgb(220, 50, 47),
                cursor_color: egui::Color32::from_rgb(88, 110, 117),
                hover_bg: egui::Color32::from_rgb(238, 228, 198),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 50),
                border: egui::Color32::from_rgb(180, 170, 140),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(255, 255, 255),
                card_hover: egui::Color32::from_rgb(248, 241, 218),
                input_bg: egui::Color32::from_rgb(255, 255, 255),
                input_border: egui::Color32::from_rgb(170, 160, 130),
                button_bg: egui::Color32::from_rgb(200, 190, 160),     // Medium gray for dark text
                badge_bg: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 25),
                menu_bg: egui::Color32::from_rgb(255, 255, 255),      // Dropdown menu background (white)
                focus_ring: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 150),
                divider: egui::Color32::from_rgb(200, 190, 160),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(253, 246, 227, 240),
                success_dim: egui::Color32::from_rgba_unmultiplied(133, 153, 0, 50),
                error_dim: egui::Color32::from_rgba_unmultiplied(220, 50, 47, 50),
            },
            ThemePreset::GitHubLight => ThemeColors {
                bg_primary: egui::Color32::from_rgb(255, 255, 255),
                bg_secondary: egui::Color32::from_rgb(246, 248, 250),
                bg_elevated: egui::Color32::from_rgb(255, 255, 255),
                fg_primary: egui::Color32::from_rgb(36, 41, 47),             // Dark text
                fg_dim: egui::Color32::from_rgb(88, 96, 105),
                accent: egui::Color32::from_rgb(31, 111, 235),
                green: egui::Color32::from_rgb(31, 136, 61),
                red: egui::Color32::from_rgb(218, 54, 51),
                cursor_color: egui::Color32::from_rgb(36, 41, 47),
                hover_bg: egui::Color32::from_rgb(240, 244, 248),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 40),
                border: egui::Color32::from_rgb(208, 215, 222),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(246, 248, 250),
                card_hover: egui::Color32::from_rgb(240, 244, 248),
                input_bg: egui::Color32::from_rgb(255, 255, 255),
                input_border: egui::Color32::from_rgb(180, 185, 190),
                button_bg: egui::Color32::from_rgb(205, 215, 225),     // Medium gray for dark text
                badge_bg: egui::Color32::from_rgba_unmultiplied(31, 111, 235, 20),
                menu_bg: egui::Color32::from_rgb(255, 255, 255),      // Dropdown menu background (white)
                focus_ring: egui::Color32::from_rgba_unmultiplied(31, 111, 235, 150),
                divider: egui::Color32::from_rgb(224, 228, 232),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
                success_dim: egui::Color32::from_rgba_unmultiplied(31, 136, 61, 50),
                error_dim: egui::Color32::from_rgba_unmultiplied(218, 54, 51, 50),
            },
            ThemePreset::OneLight => ThemeColors {
                bg_primary: egui::Color32::from_rgb(255, 255, 255),
                bg_secondary: egui::Color32::from_rgb(245, 245, 245),
                bg_elevated: egui::Color32::from_rgb(255, 255, 255),
                fg_primary: egui::Color32::from_rgb(66, 70, 85),             // Dark text
                fg_dim: egui::Color32::from_rgb(140, 143, 160),
                accent: egui::Color32::from_rgb(97, 175, 239),
                green: egui::Color32::from_rgb(88, 175, 115),
                red: egui::Color32::from_rgb(225, 109, 112),
                cursor_color: egui::Color32::from_rgb(66, 70, 85),
                hover_bg: egui::Color32::from_rgb(235, 235, 235),
                hover_shadow: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 50),
                border: egui::Color32::from_rgb(220, 220, 220),
                // Semantic colors
                card_bg: egui::Color32::from_rgb(248, 248, 248),
                card_hover: egui::Color32::from_rgb(240, 240, 240),
                input_bg: egui::Color32::from_rgb(255, 255, 255),
                input_border: egui::Color32::from_rgb(210, 210, 210),
                button_bg: egui::Color32::from_rgb(200, 200, 200),     // Medium gray for dark text
                badge_bg: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 20),
                menu_bg: egui::Color32::from_rgb(255, 255, 255),      // Dropdown menu background (white)
                focus_ring: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 150),
                divider: egui::Color32::from_rgb(230, 230, 230),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
                success_dim: egui::Color32::from_rgba_unmultiplied(88, 175, 115, 50),
                error_dim: egui::Color32::from_rgba_unmultiplied(225, 109, 112, 50),
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
            ThemePreset::SolarizedLight => "Solarized Light",
            ThemePreset::GitHubLight => "GitHub Light",
            ThemePreset::OneLight => "One Light",
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
            ThemePreset::SolarizedLight => "solarized_light",
            ThemePreset::GitHubLight => "github_light",
            ThemePreset::OneLight => "one_light",
        }
    }

    pub fn all() -> &'static [ThemePreset] {
        &[
            ThemePreset::TokyoNight,
            ThemePreset::Dracula,
            ThemePreset::OneDark,
            ThemePreset::SolarizedDark,
            ThemePreset::Nord,
            ThemePreset::SolarizedLight,
            ThemePreset::GitHubLight,
            ThemePreset::OneLight,
        ]
    }

    #[allow(dead_code)]
    pub fn from_id(id: &str) -> Self {
        match id {
            "dracula" => ThemePreset::Dracula,
            "one_dark" => ThemePreset::OneDark,
            "solarized_dark" => ThemePreset::SolarizedDark,
            "nord" => ThemePreset::Nord,
            "solarized_light" => ThemePreset::SolarizedLight,
            "github_light" => ThemePreset::GitHubLight,
            "one_light" => ThemePreset::OneLight,
            _ => ThemePreset::TokyoNight,
        }
    }
}
