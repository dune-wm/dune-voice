//! How Hex hears the phrases, learnt by `dune-voice train`: `~/.local/state/dune-voice/
//! learned.toml` (or `$XDG_STATE_HOME/dune-voice/learned.toml`), what was heard and the
//! phrase it was. Read each time dune-voice listens: a learned variant becomes its phrase
//! before anything matches.
//!
//! ```toml
//! [heard]
//! "open the valdi" = "open vivaldi"
//! "dev tools" = "devtools"
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::phrases::{self, Phrase};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Learned {
    /// What was heard (its words, as `phrases::words` makes them) → the phrase said.
    pub heard: BTreeMap<String, String>,
}

/// Where the file is.
pub fn path() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from).join(".local/state")
    });
    state.join("dune-voice/learned.toml")
}

impl Learned {
    /// The file; none (or unreadable): nothing learnt.
    pub fn load() -> Learned {
        std::fs::read_to_string(path()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// `heard` as its phrase when it is a learned variant, else as it is.
    pub fn apply<'a>(&'a self, heard: &'a str) -> &'a str {
        self.heard.get(&key(heard)).map_or(heard, String::as_str)
    }

    /// Learn what was heard when `phrase` was said: each one that is not the phrase
    /// already, nor another phrase without free text (it would take that one's place).
    /// What it did with each, to tell.
    pub fn learn(&mut self, phrase: &str, heard: &[String], table: &[Phrase]) -> Vec<(String, Kept)> {
        let said = key(phrase);
        heard
            .iter()
            .map(|text| {
                let k = key(text);
                let kept = if k.is_empty() {
                    Kept::Nothing
                } else if k == said {
                    Kept::Right
                } else if let Some(other) = phrases::fixed(table, text).filter(|p| key(&p.said) != said) {
                    Kept::Other(other.said.clone())
                } else {
                    self.heard.insert(k.clone(), said.clone());
                    Kept::Learned
                };
                (k, kept)
            })
            .collect()
    }
}

/// What `learn` did with one thing heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// Heard nothing.
    Nothing,
    /// It was the phrase.
    Right,
    /// Kept as a variant of the phrase.
    Learned,
    /// Not kept: it is this other phrase.
    Other(String),
}

/// The words of a text, as matching compares them.
fn key(text: &str) -> String {
    phrases::words(text).join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<Phrase> {
        phrases::table(&BTreeMap::new())
    }

    #[test]
    fn variants_are_learnt_and_applied() {
        let mut learned = Learned::default();
        let heard: Vec<String> =
            ["Open Vivaldi.", "open the valdi", "Oben vivaldi!", "", "close"].iter().map(|s| s.to_string()).collect();
        let kept = learned.learn("open vivaldi", &heard, &table());
        assert_eq!(kept[0].1, Kept::Right);
        assert_eq!(kept[1].1, Kept::Learned);
        assert_eq!(kept[2].1, Kept::Learned);
        assert_eq!(kept[3].1, Kept::Nothing);
        assert_eq!(kept[4].1, Kept::Other("close".into()));
        assert_eq!(learned.apply("Open the Valdi."), "open vivaldi");
        assert_eq!(learned.apply("oben vivaldi"), "open vivaldi");
        assert_eq!(learned.apply("close"), "close");
        assert_eq!(learned.apply("open firefox"), "open firefox");
    }

    #[test]
    fn the_same_words_are_one_variant() {
        // "dev tools" and "Dev-Tools" are the same words: one variant
        let mut learned = Learned::default();
        let kept = learned.learn("devtools", &["dev tools".into(), "Dev-Tools".into()], &table());
        assert!(kept.iter().all(|(_, k)| *k == Kept::Learned));
        assert_eq!(learned.heard.len(), 1);
        assert_eq!(learned.apply("dev tools"), "devtools");
    }

    #[test]
    fn the_file_roundtrips() {
        let mut learned = Learned::default();
        learned.heard.insert("dev tools".into(), "devtools".into());
        let text = toml::to_string(&learned).unwrap();
        let back: Learned = toml::from_str(&text).unwrap();
        assert_eq!(back.heard, learned.heard);
    }
}
