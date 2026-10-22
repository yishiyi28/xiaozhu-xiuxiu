//! UI translations. Document data and command identifiers stay language independent.
use crate::config::Language;
use std::{cell::Cell, collections::HashMap, sync::OnceLock};

thread_local! {
    // UI threads select their own language, so headless tests remain isolated.
    static LANGUAGE: Cell<Language> = const { Cell::new(Language::English) };
}

pub fn set_language(language: Language) {
    LANGUAGE.set(language);
}

fn chinese() -> &'static HashMap<&'static str, &'static str> {
    static MESSAGES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MESSAGES.get_or_init(|| {
        include_str!("../assets/locales/zh-CN.tsv")
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect()
    })
}

pub fn tr(text: &str) -> &str {
    if LANGUAGE.get() == Language::SimplifiedChinese {
        chinese().get(text).copied().unwrap_or(text)
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_unique_and_language_can_switch_live() {
        let mut keys = std::collections::HashSet::new();
        for line in include_str!("../assets/locales/zh-CN.tsv").lines() {
            let (key, value) = line.split_once('\t').expect("translation pair");
            assert!(!value.is_empty());
            assert!(keys.insert(key), "Duplicate translation: {key}");
        }
        set_language(Language::SimplifiedChinese);
        assert_eq!(tr("Settings"), "设置");
        assert_eq!(tr("a user's layer name"), "a user's layer name");
        set_language(Language::English);
        assert_eq!(tr("Settings"), "Settings");
    }
}
