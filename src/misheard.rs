//! What was heard and not understood, kept to be taught later (`dune-voice fix`):
//! `~/.local/state/dune-voice/misheard.jsonl`, beside `learned.toml`, a JSON line each
//! time, `{"time": seconds since 1970, "heard": "as Hex wrote it"}`. `fix` goes through
//! them, the most repeated first, and offers the closest phrases; what is taught (or
//! forgotten) leaves the file.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::phrases::{self, Phrase};

/// Where the file is.
pub fn path() -> PathBuf {
    crate::learned::path().with_file_name("misheard.jsonl")
}

#[derive(Debug, Deserialize, Serialize)]
struct Line {
    time: u64,
    heard: String,
}

/// Keep `heard`, not understood. A full disk or a missing folder must not stop listening.
pub fn log(heard: &str) {
    let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let Ok(line) = serde_json::to_string(&Line { time, heard: heard.trim().to_string() }) else { return };
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}

/// One thing not understood, however many times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// Its words, as matching compares them (and `learned.toml` keeps them).
    pub key: String,
    /// As Hex wrote it the last time.
    pub heard: String,
    pub count: usize,
    /// When last (seconds since 1970).
    pub last: u64,
}

/// The file's lines grouped by their words, the most repeated first (then the latest).
pub fn pending(text: &str) -> Vec<Pending> {
    let mut found: Vec<Pending> = Vec::new();
    for line in text.lines().filter_map(|l| serde_json::from_str::<Line>(l).ok()) {
        let key = phrases::words(&line.heard).join(" ");
        if key.is_empty() || key == "unk" {
            continue;
        }
        match found.iter_mut().find(|p| p.key == key) {
            Some(p) => {
                p.count += 1;
                if line.time >= p.last {
                    p.last = line.time;
                    p.heard = line.heard;
                }
            }
            None => found.push(Pending { key, heard: line.heard, count: 1, last: line.time }),
        }
    }
    found.sort_by(|a, b| b.count.cmp(&a.count).then(b.last.cmp(&a.last)));
    found
}

/// The file without the lines whose words are `key`.
pub fn without(text: &str, key: &str) -> String {
    text.lines()
        .filter(|l| serde_json::from_str::<Line>(l).map_or(true, |line| phrases::words(&line.heard).join(" ") != key))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// What can be said without free text: the table's phrases (`{n}` as each number said),
/// and those of the browser and the minimap (`crate::context`); one of those that differ
/// only in their spaces.
pub fn candidates(table: &[Phrase]) -> Vec<String> {
    const NUMBERS: [&str; 9] = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
    let mut out: Vec<String> = Vec::new();
    for phrase in table.iter().filter(|p| !p.free()) {
        if phrase.said.contains("{n}") {
            out.extend(NUMBERS.iter().map(|n| phrase.said.replace("{n}", n)));
        } else {
            out.push(phrase.said.clone());
        }
    }
    out.extend(crate::context::phrases().iter().map(|p| p.to_string()));
    // "devtools" and "dev tools" are one to offer
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.replace(' ', "")));
    out
}

/// The `most` candidates closest to `key`, by its letters (spaces left out, as Hex splits
/// and joins words its own way: "dip tool", "devtools") and by its words.
pub fn suggest(key: &str, candidates: &[String], most: usize) -> Vec<String> {
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let k = squeeze(key);
    let mut scored: Vec<(f64, &String)> = candidates
        .iter()
        .map(|c| {
            let letters = strsim::normalized_levenshtein(&k, &squeeze(c));
            let words = strsim::jaro_winkler(key, c);
            (letters.max(words * 0.9), c)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(most).map(|(_, c)| c.clone()).collect()
}

/// Whether a phrase written in `fix` would ever run: one of the table (with or without
/// free text) or of the browser and the minimap.
pub fn sayable(table: &[Phrase], said: &str) -> bool {
    // "dune next": the table's "next", past what is on screen.
    if let Some(rest) = crate::context::past_screen(said) {
        return phrases::fixed(table, rest).is_some() || phrases::matches(table, rest);
    }
    let key = phrases::words(said).join(" ");
    phrases::fixed(table, said).is_some()
        || phrases::matches(table, said)
        || crate::context::phrases().iter().any(|p| *p == key)
}

/// "2 h ago", from `then` to `now` (seconds since 1970).
pub fn ago(then: u64, now: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(time: u64, heard: &str) -> String {
        format!("{}\n", serde_json::to_string(&Line { time, heard: heard.into() }).unwrap())
    }

    #[test]
    fn the_most_repeated_first_by_their_words() {
        let text = [line(1, "Dip tool."), line(5, "terminal site"), line(3, "dip tool"), line(4, "[unk]"), "not json\n".into()].concat();
        let p = pending(&text);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].key.as_str(), p[0].count, p[0].last, p[0].heard.as_str()), ("dip tool", 2, 3, "dip tool"));
        assert_eq!((p[1].key.as_str(), p[1].count), ("terminal site", 1));
        let left = without(&text, "dip tool");
        assert_eq!(pending(&left).len(), 1);
        assert!(left.contains("not json"));
    }

    #[test]
    fn the_closest_phrases_are_offered() {
        let table = phrases::table(&Default::default());
        let all = candidates(&table);
        assert!(all.contains(&"desk three".to_string()) && all.contains(&"devtools".to_string()));
        assert!(!all.iter().any(|p| p.contains('{')));
        assert_eq!(suggest("dip tool", &all, 3)[0], "devtools");
        assert_eq!(suggest("desk tree", &all, 3)[0], "desk three");
        assert!(suggest("terminal right", &all, 3).contains(&"terminal".to_string()));
        assert_eq!(suggest("lef", &all, 1), ["left"]);
    }

    #[test]
    fn a_written_phrase_must_be_one_that_runs() {
        let table = phrases::table(&Default::default());
        assert!(sayable(&table, "close"));
        assert!(sayable(&table, "Open Vivaldi"));
        assert!(sayable(&table, "devtools"));
        assert!(!sayable(&table, "terminal rigth"));
    }

    #[test]
    fn how_long_ago() {
        assert_eq!(ago(100, 130), "just now");
        assert_eq!(ago(0, 7300), "2 h ago");
        assert_eq!(ago(0, 3 * 86400), "3 days ago");
    }
}
