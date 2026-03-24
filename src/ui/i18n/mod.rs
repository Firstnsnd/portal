//! # Internationalization (i18n)
//!
//! Multi-language translation support for the application.

mod en;
mod zh;
mod ja;
mod ko;
mod es;
mod ru;
mod fr;

#[derive(Clone, Copy, PartialEq)]
pub enum Language {
    English,
    Chinese,
    Japanese,
    Korean,
    Spanish,
    Russian,
    French,
}

impl Language {
    /// Translate a key to a static string.
    pub fn t(&self, key: &str) -> &'static str {
        match self {
            Language::English => en::t(key),
            Language::Chinese => zh::t(key),
            Language::Japanese => ja::t(key),
            Language::Korean => ko::t(key),
            Language::Spanish => es::t(key),
            Language::Russian => ru::t(key),
            Language::French => fr::t(key),
        }
    }

    /// Translate a key with one argument.
    pub fn tf(&self, key: &str, arg: &str) -> String {
        match self {
            Language::English => en::tf(key, arg),
            Language::Chinese => zh::tf(key, arg),
            Language::Japanese => ja::tf(key, arg),
            Language::Korean => ko::tf(key, arg),
            Language::Spanish => es::tf(key, arg),
            Language::Russian => ru::tf(key, arg),
            Language::French => fr::tf(key, arg),
        }
    }

    /// Translate a key with two arguments.
    pub fn tf2(&self, key: &str, arg1: &str, arg2: &str) -> String {
        match self {
            Language::English => en::tf2(key, arg1, arg2),
            Language::Chinese => zh::tf2(key, arg1, arg2),
            Language::Japanese => ja::tf2(key, arg1, arg2),
            Language::Korean => ko::tf2(key, arg1, arg2),
            Language::Spanish => es::tf2(key, arg1, arg2),
            Language::Russian => ru::tf2(key, arg1, arg2),
            Language::French => fr::tf2(key, arg1, arg2),
        }
    }

    /// Get the display label for this language.
    pub fn label(&self) -> &'static str {
        match self {
            Language::English => en::LABEL,
            Language::Chinese => zh::LABEL,
            Language::Japanese => ja::LABEL,
            Language::Korean => ko::LABEL,
            Language::Spanish => es::LABEL,
            Language::Russian => ru::LABEL,
            Language::French => fr::LABEL,
        }
    }

    /// Get all available languages.
    pub fn all() -> &'static [Language] {
        &[
            Language::English,
            Language::Chinese,
            Language::Japanese,
            Language::Korean,
            Language::Spanish,
            Language::Russian,
            Language::French,
        ]
    }

    /// Get the ISO 639-1 code for this language.
    pub fn id(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Chinese => "zh",
            Language::Japanese => "ja",
            Language::Korean => "ko",
            Language::Spanish => "es",
            Language::Russian => "ru",
            Language::French => "fr",
        }
    }

    /// Parse an ISO 639-1 code into a Language.
    pub fn from_id(id: &str) -> Self {
        match id {
            "zh" => Language::Chinese,
            "ja" => Language::Japanese,
            "ko" => Language::Korean,
            "es" => Language::Spanish,
            "ru" => Language::Russian,
            "fr" => Language::French,
            _ => Language::English,
        }
    }
}

/// Format a timestamp as "time ago" (e.g., "5 min ago").
pub fn format_time_ago(secs_ago: u64, lang: &Language) -> String {
    match lang {
        Language::English => en::format_time_ago(secs_ago),
        Language::Chinese => zh::format_time_ago(secs_ago),
        Language::Japanese => ja::format_time_ago(secs_ago),
        Language::Korean => ko::format_time_ago(secs_ago),
        Language::Spanish => es::format_time_ago(secs_ago),
        Language::Russian => ru::format_time_ago(secs_ago),
        Language::French => fr::format_time_ago(secs_ago),
    }
}
