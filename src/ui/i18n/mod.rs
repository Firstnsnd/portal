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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    const LANGS: [&str; 7] = ["en", "zh", "ja", "ko", "es", "ru", "fr"];

    /// Extract literal call keys like `.t("key")` / `.tf("key")` from source.
    /// The needles are mutually exclusive: `.t("` cannot match inside `.tf("`
    /// because the character after `t` differs.
    fn extract_calls(src: &str, func: &str) -> BTreeSet<String> {
        let needle = format!(".{}(\"", func);
        let mut keys = BTreeSet::new();
        let mut idx = 0;
        while let Some(pos) = src[idx..].find(&needle) {
            let start = idx + pos + needle.len();
            if let Some(end) = src[start..].find('"') {
                keys.insert(src[start..start + end].to_string());
            }
            idx = start;
        }
        keys
    }

    /// Keys registered in a language file's `t()` / `tf()` / `tf2()` table,
    /// delimited by the next `pub fn` marker (or end of file).
    fn table_keys(src: &str, func: &str) -> BTreeSet<String> {
        let start = src.find(&format!("pub fn {}(", func)).expect("table fn exists");
        let rest = &src[start..];
        let end = rest["pub fn".len()..]
            .find("\npub fn ")
            .map(|p| p + "pub fn".len())
            .unwrap_or(rest.len());
        let mut keys = BTreeSet::new();
        for line in rest[..end].lines() {
            let line = line.trim();
            if let Some(key_end) = line.strip_prefix('"') {
                if let Some(name_end) = key_end.find('"') {
                    if line.contains("=>") {
                        keys.insert(key_end[..name_end].to_string());
                    }
                }
            }
        }
        keys
    }

    fn collect_rs_files(dir: &PathBuf, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Skip the i18n module itself: its dispatch uses variable
                    // keys (no literals), and this file's own comments mention
                    // `.t("` / `.tf("` in prose.
                    if path.file_name().is_some_and(|n| n == "i18n") {
                        continue;
                    }
                    collect_rs_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
    }

    /// Every literal `.t()` / `.tf()` / `.tf2()` key used in the crate must be
    /// registered in the matching table of every language — otherwise the
    /// UI renders the "???" fallback (or a literal `key: arg`), which is how
    /// `session_ended` slipped out as "???" on the local terminal overlay.
    #[test]
    fn all_used_keys_exist_in_every_language_table() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_rs_files(&manifest.join("src"), &mut files);

        let mut used_t = BTreeSet::new();
        let mut used_tf = BTreeSet::new();
        let mut used_tf2 = BTreeSet::new();
        for file in &files {
            let src = std::fs::read_to_string(file).expect("readable source file");
            used_t.extend(extract_calls(&src, "t"));
            used_tf.extend(extract_calls(&src, "tf"));
            used_tf2.extend(extract_calls(&src, "tf2"));
        }

        for lang in LANGS {
            let src = std::fs::read_to_string(
                manifest.join("src/ui/i18n").join(format!("{}.rs", lang)),
            )
            .expect("language file readable");

            let t_keys = table_keys(&src, "t");
            let tf_keys = table_keys(&src, "tf");
            let tf2_keys = table_keys(&src, "tf2");
            let missing_t: Vec<_> = used_t.difference(&t_keys).collect();
            let missing_tf: Vec<_> = used_tf.difference(&tf_keys).collect();
            let missing_tf2: Vec<_> = used_tf2.difference(&tf2_keys).collect();

            assert!(
                missing_t.is_empty() && missing_tf.is_empty() && missing_tf2.is_empty(),
                "[{}] missing i18n keys — t(): {:?}, tf(): {:?}, tf2(): {:?}",
                lang, missing_t, missing_tf, missing_tf2
            );
        }
    }
}
