use eframe::egui;
use crate::config::{KeyBinding, ShortcutAction};

pub struct ShortcutResolver {
    bindings: Vec<KeyBinding>,
}

impl ShortcutResolver {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        Self { bindings }
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    pub fn update_bindings(&mut self, bindings: Vec<KeyBinding>) {
        self.bindings = bindings;
    }

    pub fn matches(&self, action: ShortcutAction, ctx: &egui::Context) -> bool {
        for binding in &self.bindings {
            if binding.action != action {
                continue;
            }
            if let Some(key) = Self::key_from_string(&binding.key) {
                let pressed = ctx.input(|i| {
                    i.key_pressed(key)
                        && i.modifiers.command == binding.command
                        && i.modifiers.shift == binding.shift
                        && i.modifiers.alt == binding.alt
                        && i.modifiers.ctrl == binding.ctrl
                });
                if pressed {
                    return true;
                }
            }
        }
        false
    }

    pub fn key_from_string(s: &str) -> Option<egui::Key> {
        match s {
            "A" => Some(egui::Key::A),
            "B" => Some(egui::Key::B),
            "C" => Some(egui::Key::C),
            "D" => Some(egui::Key::D),
            "E" => Some(egui::Key::E),
            "F" => Some(egui::Key::F),
            "G" => Some(egui::Key::G),
            "H" => Some(egui::Key::H),
            "I" => Some(egui::Key::I),
            "J" => Some(egui::Key::J),
            "K" => Some(egui::Key::K),
            "L" => Some(egui::Key::L),
            "M" => Some(egui::Key::M),
            "N" => Some(egui::Key::N),
            "O" => Some(egui::Key::O),
            "P" => Some(egui::Key::P),
            "Q" => Some(egui::Key::Q),
            "R" => Some(egui::Key::R),
            "S" => Some(egui::Key::S),
            "T" => Some(egui::Key::T),
            "U" => Some(egui::Key::U),
            "V" => Some(egui::Key::V),
            "W" => Some(egui::Key::W),
            "X" => Some(egui::Key::X),
            "Y" => Some(egui::Key::Y),
            "Z" => Some(egui::Key::Z),
            "0" | "Num0" => Some(egui::Key::Num0),
            "1" | "Num1" => Some(egui::Key::Num1),
            "2" | "Num2" => Some(egui::Key::Num2),
            "3" | "Num3" => Some(egui::Key::Num3),
            "4" | "Num4" => Some(egui::Key::Num4),
            "5" | "Num5" => Some(egui::Key::Num5),
            "6" | "Num6" => Some(egui::Key::Num6),
            "7" | "Num7" => Some(egui::Key::Num7),
            "8" | "Num8" => Some(egui::Key::Num8),
            "9" | "Num9" => Some(egui::Key::Num9),
            "F1" => Some(egui::Key::F1),
            "F2" => Some(egui::Key::F2),
            "F3" => Some(egui::Key::F3),
            "F4" => Some(egui::Key::F4),
            "F5" => Some(egui::Key::F5),
            "F6" => Some(egui::Key::F6),
            "F7" => Some(egui::Key::F7),
            "F8" => Some(egui::Key::F8),
            "F9" => Some(egui::Key::F9),
            "F10" => Some(egui::Key::F10),
            "F11" => Some(egui::Key::F11),
            "F12" => Some(egui::Key::F12),
            "Space" => Some(egui::Key::Space),
            "Enter" => Some(egui::Key::Enter),
            "Escape" => Some(egui::Key::Escape),
            "Tab" => Some(egui::Key::Tab),
            "Backspace" => Some(egui::Key::Backspace),
            "Delete" => Some(egui::Key::Delete),
            "Insert" => Some(egui::Key::Insert),
            "Home" => Some(egui::Key::Home),
            "End" => Some(egui::Key::End),
            "PageUp" => Some(egui::Key::PageUp),
            "PageDown" => Some(egui::Key::PageDown),
            "ArrowUp" => Some(egui::Key::ArrowUp),
            "ArrowDown" => Some(egui::Key::ArrowDown),
            "ArrowLeft" => Some(egui::Key::ArrowLeft),
            "ArrowRight" => Some(egui::Key::ArrowRight),
            "Minus" => Some(egui::Key::Minus),
            "Plus" => Some(egui::Key::Plus),
            "Equals" => Some(egui::Key::Equals),
            "LeftBracket" | "OpenBracket" => Some(egui::Key::OpenBracket),
            "RightBracket" | "CloseBracket" => Some(egui::Key::CloseBracket),
            "Backslash" => Some(egui::Key::Backslash),
            "Semicolon" => Some(egui::Key::Semicolon),
            "Colon" => Some(egui::Key::Colon),
            "Quote" => Some(egui::Key::Quote),
            "Comma" => Some(egui::Key::Comma),
            "Period" => Some(egui::Key::Period),
            "Slash" => Some(egui::Key::Slash),
            "Backtick" => Some(egui::Key::Backtick),
            "Pipe" => Some(egui::Key::Pipe),
            _ => None,
        }
    }

    pub fn key_to_string(key: egui::Key) -> String {
        match key {
            egui::Key::A => "A".into(), egui::Key::B => "B".into(), egui::Key::C => "C".into(),
            egui::Key::D => "D".into(), egui::Key::E => "E".into(), egui::Key::F => "F".into(),
            egui::Key::G => "G".into(), egui::Key::H => "H".into(), egui::Key::I => "I".into(),
            egui::Key::J => "J".into(), egui::Key::K => "K".into(), egui::Key::L => "L".into(),
            egui::Key::M => "M".into(), egui::Key::N => "N".into(), egui::Key::O => "O".into(),
            egui::Key::P => "P".into(), egui::Key::Q => "Q".into(), egui::Key::R => "R".into(),
            egui::Key::S => "S".into(), egui::Key::T => "T".into(), egui::Key::U => "U".into(),
            egui::Key::V => "V".into(), egui::Key::W => "W".into(), egui::Key::X => "X".into(),
            egui::Key::Y => "Y".into(), egui::Key::Z => "Z".into(),
            egui::Key::Num0 => "0".into(), egui::Key::Num1 => "1".into(),
            egui::Key::Num2 => "2".into(), egui::Key::Num3 => "3".into(),
            egui::Key::Num4 => "4".into(), egui::Key::Num5 => "5".into(),
            egui::Key::Num6 => "6".into(), egui::Key::Num7 => "7".into(),
            egui::Key::Num8 => "8".into(), egui::Key::Num9 => "9".into(),
            egui::Key::F1 => "F1".into(), egui::Key::F2 => "F2".into(),
            egui::Key::F3 => "F3".into(), egui::Key::F4 => "F4".into(),
            egui::Key::F5 => "F5".into(), egui::Key::F6 => "F6".into(),
            egui::Key::F7 => "F7".into(), egui::Key::F8 => "F8".into(),
            egui::Key::F9 => "F9".into(), egui::Key::F10 => "F10".into(),
            egui::Key::F11 => "F11".into(), egui::Key::F12 => "F12".into(),
            egui::Key::Space => "Space".into(), egui::Key::Enter => "Enter".into(),
            egui::Key::Escape => "Escape".into(), egui::Key::Tab => "Tab".into(),
            egui::Key::Backspace => "Backspace".into(), egui::Key::Delete => "Delete".into(),
            egui::Key::Insert => "Insert".into(), egui::Key::Home => "Home".into(),
            egui::Key::End => "End".into(), egui::Key::PageUp => "PageUp".into(),
            egui::Key::PageDown => "PageDown".into(),
            egui::Key::ArrowUp => "ArrowUp".into(), egui::Key::ArrowDown => "ArrowDown".into(),
            egui::Key::ArrowLeft => "ArrowLeft".into(), egui::Key::ArrowRight => "ArrowRight".into(),
            egui::Key::Minus => "Minus".into(), egui::Key::Plus => "Plus".into(),
            egui::Key::Equals => "Equals".into(),
            egui::Key::OpenBracket => "LeftBracket".into(), egui::Key::CloseBracket => "RightBracket".into(),
            egui::Key::Backslash => "Backslash".into(), egui::Key::Semicolon => "Semicolon".into(),
            egui::Key::Colon => "Colon".into(), egui::Key::Quote => "Quote".into(),
            egui::Key::Comma => "Comma".into(), egui::Key::Period => "Period".into(),
            egui::Key::Slash => "Slash".into(), egui::Key::Backtick => "Backtick".into(),
            egui::Key::Pipe => "Pipe".into(),
            _ => format!("{:?}", key),
        }
    }

    pub fn display_binding(binding: &KeyBinding) -> String {
        let mut parts = Vec::new();
        if binding.ctrl { parts.push("\u{2303}"); }
        if binding.alt { parts.push("\u{2325}"); }
        if binding.shift { parts.push("\u{21e7}"); }
        if binding.command { parts.push("\u{2318}"); }
        let key_display = match binding.key.as_str() {
            "LeftBracket" | "OpenBracket" => "[".to_string(),
            "RightBracket" | "CloseBracket" => "]".to_string(),
            "Backslash" => "\\".to_string(),
            "Semicolon" => ";".to_string(),
            "Quote" => "'".to_string(),
            "Comma" => ",".to_string(),
            "Period" => ".".to_string(),
            "Slash" => "/".to_string(),
            "Backtick" => "`".to_string(),
            "Minus" => "-".to_string(),
            "Plus" => "+".to_string(),
            "Equals" => "=".to_string(),
            "Space" => "Space".to_string(),
            "ArrowUp" => "\u{2191}".to_string(),
            "ArrowDown" => "\u{2193}".to_string(),
            "ArrowLeft" => "\u{2190}".to_string(),
            "ArrowRight" => "\u{2192}".to_string(),
            other => other.to_string(),
        };
        parts.push(&key_display);
        // We must collect String values then join
        parts.join("")
    }
}

/// Map egui Key to Ctrl+key byte (0x01-0x1A)
pub fn key_to_ctrl_byte(key: &egui::Key) -> Option<u8> {
    match key {
        egui::Key::A => Some(0x01),
        egui::Key::B => Some(0x02),
        egui::Key::C => Some(0x03),
        egui::Key::D => Some(0x04),
        egui::Key::E => Some(0x05),
        egui::Key::F => Some(0x06),
        egui::Key::G => Some(0x07),
        egui::Key::H => Some(0x08),
        egui::Key::I => Some(0x09),
        egui::Key::J => Some(0x0A),
        egui::Key::K => Some(0x0B),
        egui::Key::L => Some(0x0C),
        egui::Key::M => Some(0x0D),
        egui::Key::N => Some(0x0E),
        egui::Key::O => Some(0x0F),
        egui::Key::P => Some(0x10),
        egui::Key::Q => Some(0x11),
        egui::Key::R => Some(0x12),
        egui::Key::S => Some(0x13),
        egui::Key::T => Some(0x14),
        egui::Key::U => Some(0x15),
        egui::Key::V => Some(0x16),
        egui::Key::W => Some(0x17),
        egui::Key::X => Some(0x18),
        egui::Key::Y => Some(0x19),
        egui::Key::Z => Some(0x1A),
        _ => None,
    }
}

/// Map egui Key to printable character (fallback when Event::Text is missing)
pub fn key_to_char(key: &egui::Key, shift: bool) -> Option<char> {
    match key {
        // Letters
        egui::Key::A => Some(if shift { 'A' } else { 'a' }),
        egui::Key::B => Some(if shift { 'B' } else { 'b' }),
        egui::Key::C => Some(if shift { 'C' } else { 'c' }),
        egui::Key::D => Some(if shift { 'D' } else { 'd' }),
        egui::Key::E => Some(if shift { 'E' } else { 'e' }),
        egui::Key::F => Some(if shift { 'F' } else { 'f' }),
        egui::Key::G => Some(if shift { 'G' } else { 'g' }),
        egui::Key::H => Some(if shift { 'H' } else { 'h' }),
        egui::Key::I => Some(if shift { 'I' } else { 'i' }),
        egui::Key::J => Some(if shift { 'J' } else { 'j' }),
        egui::Key::K => Some(if shift { 'K' } else { 'k' }),
        egui::Key::L => Some(if shift { 'L' } else { 'l' }),
        egui::Key::M => Some(if shift { 'M' } else { 'm' }),
        egui::Key::N => Some(if shift { 'N' } else { 'n' }),
        egui::Key::O => Some(if shift { 'O' } else { 'o' }),
        egui::Key::P => Some(if shift { 'P' } else { 'p' }),
        egui::Key::Q => Some(if shift { 'Q' } else { 'q' }),
        egui::Key::R => Some(if shift { 'R' } else { 'r' }),
        egui::Key::S => Some(if shift { 'S' } else { 's' }),
        egui::Key::T => Some(if shift { 'T' } else { 't' }),
        egui::Key::U => Some(if shift { 'U' } else { 'u' }),
        egui::Key::V => Some(if shift { 'V' } else { 'v' }),
        egui::Key::W => Some(if shift { 'W' } else { 'w' }),
        egui::Key::X => Some(if shift { 'X' } else { 'x' }),
        egui::Key::Y => Some(if shift { 'Y' } else { 'y' }),
        egui::Key::Z => Some(if shift { 'Z' } else { 'z' }),
        // Numbers
        egui::Key::Num0 => Some(if shift { ')' } else { '0' }),
        egui::Key::Num1 => Some(if shift { '!' } else { '1' }),
        egui::Key::Num2 => Some(if shift { '@' } else { '2' }),
        egui::Key::Num3 => Some(if shift { '#' } else { '3' }),
        egui::Key::Num4 => Some(if shift { '$' } else { '4' }),
        egui::Key::Num5 => Some(if shift { '%' } else { '5' }),
        egui::Key::Num6 => Some(if shift { '^' } else { '6' }),
        egui::Key::Num7 => Some(if shift { '&' } else { '7' }),
        egui::Key::Num8 => Some(if shift { '*' } else { '8' }),
        egui::Key::Num9 => Some(if shift { '(' } else { '9' }),
        // Punctuation
        egui::Key::Space => Some(' '),
        egui::Key::Minus => Some(if shift { '_' } else { '-' }),
        egui::Key::Plus => Some('+'),
        egui::Key::Equals => Some(if shift { '+' } else { '=' }),
        egui::Key::OpenBracket => Some(if shift { '{' } else { '[' }),
        egui::Key::CloseBracket => Some(if shift { '}' } else { ']' }),
        egui::Key::Backslash => Some(if shift { '|' } else { '\\' }),
        egui::Key::Semicolon => Some(if shift { ':' } else { ';' }),
        egui::Key::Colon => Some(':'),
        egui::Key::Quote => Some(if shift { '"' } else { '\'' }),
        egui::Key::Comma => Some(if shift { '<' } else { ',' }),
        egui::Key::Period => Some(if shift { '>' } else { '.' }),
        egui::Key::Slash => Some(if shift { '?' } else { '/' }),
        egui::Key::Backtick => Some(if shift { '~' } else { '`' }),
        egui::Key::Pipe => Some('|'),
        egui::Key::Questionmark => Some('?'),
        _ => None,
    }
}
