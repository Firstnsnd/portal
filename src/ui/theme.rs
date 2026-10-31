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
    pub input_bg: egui::Color32,        // Text input background
    pub input_border: egui::Color32,    // Text input border
    pub badge_bg: egui::Color32,        // Tag / badge background
    pub menu_bg: egui::Color32,         // Dropdown menu / popup background
    pub focus_ring: egui::Color32,      // Focus indicator
    pub divider: egui::Color32,         // Section divider line
    pub overlay_bg: egui::Color32,      // Modal overlay background
    pub error_dim: egui::Color32,       // Dimmed error (for backgrounds)
    pub warning: egui::Color32,         // Warning/attention (notifications, in-use state)
    pub search_match: egui::Color32,     // Search match highlight (non-current)
    pub search_match_current: egui::Color32, // Search match highlight (current)
    pub button_text: egui::Color32,     // Contrast text on filled buttons (primary/danger)
    pub broadcast_bg: egui::Color32,    // Broadcast-enabled tab fill
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
                input_bg: egui::Color32::from_rgb(20, 21, 36),
                input_border: egui::Color32::from_rgb(56, 60, 79),
                badge_bg: egui::Color32::from_rgba_unmultiplied(122, 162, 247, 20),
                menu_bg: egui::Color32::from_rgb(32, 34, 48),          // Dropdown menu background
                focus_ring: egui::Color32::from_rgba_unmultiplied(122, 162, 247, 120),
                divider: egui::Color32::from_rgb(45, 47, 65),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(26, 27, 38, 220),
                error_dim: egui::Color32::from_rgba_unmultiplied(247, 118, 142, 30),
                warning: egui::Color32::from_rgb(234, 179, 8),
                search_match: egui::Color32::from_rgb(255, 215, 0),
                search_match_current: egui::Color32::from_rgb(255, 140, 0),
                button_text: egui::Color32::from_rgb(255, 255, 255),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(60, 40, 100, 255),
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
                input_bg: egui::Color32::from_rgb(34, 34, 58),
                input_border: egui::Color32::from_rgb(75, 76, 97),
                badge_bg: egui::Color32::from_rgba_unmultiplied(189, 147, 249, 20),
                menu_bg: egui::Color32::from_rgb(48, 50, 68),          // Dropdown menu background
                focus_ring: egui::Color32::from_rgba_unmultiplied(189, 147, 249, 120),
                divider: egui::Color32::from_rgb(62, 64, 85),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(40, 42, 54, 220),
                error_dim: egui::Color32::from_rgba_unmultiplied(255, 85, 85, 30),
                warning: egui::Color32::from_rgb(241, 250, 140),
                search_match: egui::Color32::from_rgb(255, 215, 0),
                search_match_current: egui::Color32::from_rgb(255, 140, 0),
                button_text: egui::Color32::from_rgb(248, 248, 242),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(80, 50, 120, 255),
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
                input_bg: egui::Color32::from_rgb(23, 27, 33),
                input_border: egui::Color32::from_rgb(70, 75, 85),
                badge_bg: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 20),
                menu_bg: egui::Color32::from_rgb(46, 50, 60),          // Dropdown menu background
                focus_ring: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 120),
                divider: egui::Color32::from_rgb(58, 63, 73),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(40, 44, 52, 220),
                error_dim: egui::Color32::from_rgba_unmultiplied(224, 108, 117, 30),
                warning: egui::Color32::from_rgb(229, 181, 103),
                search_match: egui::Color32::from_rgb(255, 215, 0),
                search_match_current: egui::Color32::from_rgb(255, 140, 0),
                button_text: egui::Color32::from_rgb(255, 255, 255),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(60, 50, 100, 255),
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
                input_bg: egui::Color32::from_rgb(0, 44, 56),
                input_border: egui::Color32::from_rgb(34, 85, 98),
                badge_bg: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 20),
                menu_bg: egui::Color32::from_rgb(10, 58, 70),          // Dropdown menu background
                focus_ring: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 120),
                divider: egui::Color32::from_rgb(18, 68, 80),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(0, 43, 54, 220),
                error_dim: egui::Color32::from_rgba_unmultiplied(220, 50, 47, 30),
                warning: egui::Color32::from_rgb(181, 137, 0),
                search_match: egui::Color32::from_rgb(220, 180, 0),
                search_match_current: egui::Color32::from_rgb(220, 120, 0),
                button_text: egui::Color32::from_rgb(253, 246, 227),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(40, 30, 80, 255),
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
                input_bg: egui::Color32::from_rgb(49, 56, 72),
                input_border: egui::Color32::from_rgb(87, 96, 114),
                badge_bg: egui::Color32::from_rgba_unmultiplied(136, 192, 208, 20),
                menu_bg: egui::Color32::from_rgb(54, 62, 76),          // Dropdown menu background
                focus_ring: egui::Color32::from_rgba_unmultiplied(136, 192, 208, 120),
                divider: egui::Color32::from_rgb(76, 84, 100),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(46, 52, 64, 220),
                error_dim: egui::Color32::from_rgba_unmultiplied(191, 97, 106, 30),
                warning: egui::Color32::from_rgb(208, 135, 0),
                search_match: egui::Color32::from_rgb(220, 180, 0),
                search_match_current: egui::Color32::from_rgb(220, 120, 0),
                button_text: egui::Color32::from_rgb(236, 239, 244),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(80, 70, 130, 255),
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
                input_bg: egui::Color32::from_rgb(255, 255, 255),
                input_border: egui::Color32::from_rgb(170, 160, 130),
                badge_bg: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 25),
                menu_bg: egui::Color32::from_rgb(255, 255, 255),      // Dropdown menu background (white)
                focus_ring: egui::Color32::from_rgba_unmultiplied(38, 139, 210, 150),
                divider: egui::Color32::from_rgb(200, 190, 160),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(253, 246, 227, 240),
                error_dim: egui::Color32::from_rgba_unmultiplied(220, 50, 47, 50),
                warning: egui::Color32::from_rgb(180, 130, 0),
                search_match: egui::Color32::from_rgb(255, 210, 0),
                search_match_current: egui::Color32::from_rgb(255, 140, 0),
                button_text: egui::Color32::from_rgb(255, 255, 255),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(80, 60, 140, 255),
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
                input_bg: egui::Color32::from_rgb(255, 255, 255),
                input_border: egui::Color32::from_rgb(180, 185, 190),
                badge_bg: egui::Color32::from_rgba_unmultiplied(31, 111, 235, 20),
                menu_bg: egui::Color32::from_rgb(255, 255, 255),      // Dropdown menu background (white)
                focus_ring: egui::Color32::from_rgba_unmultiplied(31, 111, 235, 150),
                divider: egui::Color32::from_rgb(224, 228, 232),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
                error_dim: egui::Color32::from_rgba_unmultiplied(218, 54, 51, 50),
                warning: egui::Color32::from_rgb(210, 150, 0),
                search_match: egui::Color32::from_rgb(255, 215, 0),
                search_match_current: egui::Color32::from_rgb(255, 140, 0),
                button_text: egui::Color32::from_rgb(255, 255, 255),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(70, 50, 130, 255),
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
                input_bg: egui::Color32::from_rgb(255, 255, 255),
                input_border: egui::Color32::from_rgb(210, 210, 210),
                badge_bg: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 20),
                menu_bg: egui::Color32::from_rgb(255, 255, 255),      // Dropdown menu background (white)
                focus_ring: egui::Color32::from_rgba_unmultiplied(97, 175, 239, 150),
                divider: egui::Color32::from_rgb(230, 230, 230),
                overlay_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
                error_dim: egui::Color32::from_rgba_unmultiplied(225, 109, 112, 50),
                warning: egui::Color32::from_rgb(210, 150, 0),
                search_match: egui::Color32::from_rgb(255, 215, 0),
                search_match_current: egui::Color32::from_rgb(255, 140, 0),
                button_text: egui::Color32::from_rgb(255, 255, 255),
                broadcast_bg: egui::Color32::from_rgba_unmultiplied(70, 50, 130, 255),
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
}
