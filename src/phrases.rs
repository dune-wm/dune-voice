//! What can be said and what each phrase runs: the built-in English phrases, then those of
//! `voice.toml`'s `[phrases]` on top (the same phrase: the file's command; `"none"`: gone).
//! - `{n}` in a phrase is a number from one to nine ("desk three", "desk 3"), and in its
//!   command that number as a digit (`dune action desk_3`).
//! - `{text}` in a phrase is whatever is said there ("open firefox"), and in its command
//!   that text, quoted for the shell (`dune find --pick apps 'firefox'`). Only Hex hears
//!   it: Vosk only hears the phrases it is given.

use std::collections::BTreeMap;

/// One thing that can be said, as Vosk is told it, and the command it runs (`sh -c`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    pub said: String,
    pub command: String,
}

impl Phrase {
    /// Whether it has free text (`{text}`): Hex can hear it, Vosk cannot.
    pub fn free(&self) -> bool {
        self.said.contains(TEXT)
    }
}

/// What was heard, as a phrase to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard<'a> {
    pub phrase: &'a Phrase,
    /// The phrase's command, `{text}` filled in.
    pub command: String,
}

const TEXT: &str = "{text}";

/// The numbers `{n}` stands for, as they are said, and as they are sometimes heard.
const NUMBERS: [&[&str]; 9] = [
    &["one", "won"],
    &["two", "to", "too"],
    &["three", "tree"],
    &["four", "for"],
    &["five"],
    &["six"],
    &["seven"],
    &["eight", "ate"],
    &["nine"],
];

/// The built-in phrases: dune's keys, by their `dune action` names, and dune-finder.
const DEFAULTS: &[(&str, &str)] = &[
    ("desk {n}", "dune action desk_{n}"),
    ("send to {n}", "dune action send_to_desk_{n}"),
    ("left", "dune action focus_left"),
    ("right", "dune action focus_right"),
    ("up", "dune action focus_up"),
    ("down", "dune action focus_down"),
    ("move left", "dune action swap_left"),
    ("move right", "dune action swap_right"),
    ("move up", "dune action swap_up"),
    ("move down", "dune action swap_down"),
    ("monitor", "dune action focus_monitor_next"),
    ("monitor left", "dune action focus_monitor_left"),
    ("monitor right", "dune action focus_monitor_right"),
    ("send monitor", "dune action move_monitor_next"),
    ("close", "dune action close"),
    ("fullscreen", "dune action fullscreen"),
    ("full screen", "dune action fullscreen"),
    ("full width", "dune action full_width"),
    ("wider", "dune action wider"),
    ("narrower", "dune action narrower"),
    ("taller", "dune action taller"),
    ("shorter", "dune action shorter"),
    ("cycle width", "dune action cycle_width"),
    ("center", "dune action center"),
    ("float", "dune action float"),
    ("terminal", "dune action terminal_vertical"),
    ("terminal right", "dune action terminal_vertical"),
    ("terminal left", "dune action terminal_left"),
    ("terminal below", "dune action terminal_horizontal"),
    ("terminal bottom", "dune action terminal_horizontal"),
    ("terminal down", "dune action terminal_horizontal"),
    ("terminal above", "dune action terminal_above"),
    ("terminal top", "dune action terminal_above"),
    ("terminal up", "dune action terminal_above"),
    ("apps", "dune action launcher"),
    ("finder", "dune action launcher"),
    // the `/` mode: every window open (what the user calls its apps)
    ("finder apps", "dune find windows"),
    ("finder files", "dune find files"),
    ("windows", "dune action switcher"),
    ("options", "dune action tile_options"),
    ("next tab", "dune action next_tab"),
    ("previous tab", "dune action prev_tab"),
    ("next", "dune action next_tab"),
    ("back", "dune action prev_tab"),
    ("tabs", "dune action stack_to_tabs"),
    ("new tab", "dune action empty_tab"),
    ("dock", "dune action dock"),
    ("map", "dune action minimap"),
    ("mute", "dune action mute_tile"),
    ("screenshot", "dune action screenshot_screen"),
    // The pointer's buttons where it is (dune-eye-tracker moves it, and learns from these
    // clicks as from the mouse's where you look).
    ("click", "dune pointer click"),
    ("right click", "dune pointer click right"),
    ("middle click", "dune pointer click middle"),
    ("double click", "dune pointer click && dune pointer click"),
    ("lock", "dune action lock"),
    ("dictate english", "dune-voice language en"),
    ("dictate spanish", "dune-voice language es"),
    ("dictate italian", "dune-voice language it"),
    // dune-finder: the first app / window / file it lists for what is said, or the finder
    // with it typed in.
    ("open {text}", "dune find --pick apps {text}"),
    ("find {text}", "dune find apps {text}"),
    ("search {text}", "dune find apps {text}"),
    ("window {text}", "dune find --pick windows {text}"),
    ("go to {text}", "dune find --pick windows {text}"),
    ("file {text}", "dune find files {text}"),
    ("open file {text}", "dune find --pick files {text}"),
    // `:t`: the app as a tab of the focused tile's container.
    ("new tab {text}", "dune find --pick apps {text} :t"),
];

/// The sides a program can open on, as said, and as dune-finder's `:` word: "firefox
/// left", "open firefox left" (a split of the focused tile).
const SIDES: &[(&str, &str)] = &[
    ("left", "left"),
    ("right", "right"),
    ("top", "up"),
    ("up", "up"),
    ("above", "up"),
    ("bottom", "down"),
    ("down", "down"),
    ("below", "down"),
];

/// The built-in phrases with `file` on top, `{n}` spread out, in the order of the phrases.
pub fn table(file: &BTreeMap<String, String>) -> Vec<Phrase> {
    let mut templates: BTreeMap<String, String> =
        DEFAULTS.iter().map(|(said, command)| (clean(said), command.to_string())).collect();
    for (side, word) in SIDES {
        for said in [format!("{TEXT} {side}"), format!("open {TEXT} {side}")] {
            templates.insert(said, format!("dune find --pick apps {TEXT} :{word}"));
        }
    }
    for (said, command) in file {
        let said = clean(said);
        if command.trim().eq_ignore_ascii_case("none") {
            templates.remove(&said);
        } else if !said.is_empty() {
            templates.insert(said, command.clone());
        }
    }
    let mut phrases = Vec::new();
    for (said, command) in templates {
        if said.contains("{n}") {
            for (i, words) in NUMBERS.iter().enumerate() {
                phrases.push(Phrase {
                    said: said.replace("{n}", words[0]),
                    command: command.replace("{n}", &(i + 1).to_string()),
                });
            }
        } else {
            phrases.push(Phrase { said, command });
        }
    }
    phrases
}

/// The phrase `heard` is (Hex's or Vosk's text): the case, accents and punctuation not
/// counting, a number said as a word or a digit. A phrase without `{text}` first, then the
/// one with `{text}` whose words around it are the most ("open file notes": `open file
/// {text}`, not `open {text}`).
#[cfg(test)]
pub fn find<'a>(phrases: &'a [Phrase], heard: &str) -> Option<Heard<'a>> {
    find_with(phrases, heard, &|text: &str| text.to_string())
}

/// `find`, with the `{text}` of a phrase that opens an app (`dune find --pick apps`) made
/// the name of the app it is closest to (`crate::apps`): "open spotify fast", spotifast.
pub fn find_app<'a>(phrases: &'a [Phrase], heard: &str) -> Option<Heard<'a>> {
    find_with(phrases, heard, &crate::apps::resolve)
}

/// The command a phrase that opens an app starts with.
const OPENS_APP: &str = "dune find --pick apps ";

fn find_with<'a>(phrases: &'a [Phrase], heard: &str, app: &dyn Fn(&str) -> String) -> Option<Heard<'a>> {
    let heard = words(heard);
    if heard.is_empty() {
        return None;
    }
    if let Some(phrase) = phrases.iter().filter(|p| !p.free()).find(|p| same(&words(&p.said), &heard)) {
        return Some(Heard { phrase, command: phrase.command.clone() });
    }
    phrases
        .iter()
        .filter(|p| p.free())
        .filter_map(|phrase| {
            let said = words(&phrase.said);
            let at = said.iter().position(|w| w == TEXT)?;
            let (before, after) = (&said[..at], &said[at + 1..]);
            if heard.len() <= before.len() + after.len()
                || !same(before, &heard[..before.len()])
                || !same(after, &heard[heard.len() - after.len()..])
            {
                return None;
            }
            let text = heard[before.len()..heard.len() - after.len()].join(" ");
            Some((before.len() + after.len(), phrase, text))
        })
        .max_by_key(|(around, _, _)| *around)
        .map(|(_, phrase, text)| {
            // Only the one chosen looks for its app (the desktop entries are read then).
            let text = if phrase.command.starts_with(OPENS_APP) { app(&text) } else { text };
            Heard { phrase, command: phrase.command.replace(TEXT, &quote(&text)) }
        })
}

/// The phrase without free text `heard` is, if any.
pub fn fixed<'a>(phrases: &'a [Phrase], heard: &str) -> Option<&'a Phrase> {
    let heard = words(heard);
    phrases.iter().filter(|p| !p.free()).find(|p| same(&words(&p.said), &heard))
}

/// Word by word the same, a number as a word, a digit or how it is sometimes heard.
fn same(said: &[String], heard: &[String]) -> bool {
    said.len() == heard.len()
        && said.iter().zip(heard).all(|(s, h)| s == h || number(s).is_some_and(|n| number(h) == Some(n)))
}

/// The number from one to nine a word is.
fn number(word: &str) -> Option<usize> {
    match word.parse::<usize>() {
        Ok(n @ 1..=9) => Some(n),
        Ok(_) => None,
        Err(_) => NUMBERS.iter().position(|words| words.contains(&word)).map(|i| i + 1),
    }
}

/// `text` in single quotes, for `sh -c`.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A phrase as Vosk is given it: lowercase words, one space between them, nothing but
/// letters (a quote would break the grammar Vosk is given) and the `{n}` and `{text}`
/// placeholders.
fn clean(said: &str) -> String {
    let said = said.to_lowercase().replace("{n}", "\u{0}").replace(TEXT, "\u{1}");
    let kept: String = said.chars().map(|c| if c.is_alphabetic() || c < '\u{2}' { c } else { ' ' }).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ").replace('\u{0}', "{n}").replace('\u{1}', TEXT)
}

/// Lowercase words without their accents or punctuation, to compare what was heard with a
/// phrase; `{text}` kept as it is.
pub fn words(text: &str) -> Vec<String> {
    let unaccented: String = text
        .to_lowercase()
        .replace(TEXT, "\u{1}")
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            'ñ' => 'n',
            '\u{1}' => '\u{1}',
            // "it's" is one word, "dune-finder" two.
            '\'' | '’' => '\u{2}',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    unaccented
        .replace('\u{2}', "")
        .split_whitespace()
        .map(|w| if w == "\u{1}" { TEXT.to_string() } else { w.to_string() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn command(phrases: &[Phrase], heard: &str) -> Option<String> {
        find(phrases, heard).map(|h| h.command)
    }

    #[test]
    fn a_phrase_that_opens_an_app_gets_the_app_named() {
        let phrases = table(&BTreeMap::new());
        let app = |t: &str| if t == "spotify fast" { "spotifast".to_string() } else { t.to_string() };
        let heard = find_with(&phrases, "Open Spotify Fast.", &app).unwrap();
        assert_eq!(heard.command, "dune find --pick apps 'spotifast'");
        let heard = find_with(&phrases, "spotify fast left", &app).unwrap();
        assert_eq!(heard.command, "dune find --pick apps 'spotifast' :left");
        // Not for finding, files or windows.
        let heard = find_with(&phrases, "find spotify fast", &app).unwrap();
        assert_eq!(heard.command, "dune find apps 'spotify fast'");
        let heard = find_with(&phrases, "go to spotify fast", &app).unwrap();
        assert_eq!(heard.command, "dune find --pick windows 'spotify fast'");
    }

    #[test]
    fn numbers_are_spread_out_and_heard_as_words_or_digits() {
        let phrases = table(&BTreeMap::new());
        assert_eq!(command(&phrases, "desk three").as_deref(), Some("dune action desk_3"));
        assert_eq!(command(&phrases, "Desk 3.").as_deref(), Some("dune action desk_3"));
        assert_eq!(command(&phrases, "Send to 9").as_deref(), Some("dune action send_to_desk_9"));
        assert_eq!(command(&phrases, "desk to").as_deref(), Some("dune action desk_2"));
        assert_eq!(command(&phrases, "desk 12"), None);
        assert!(!phrases.iter().any(|p| p.said.contains("{n}")));
    }

    #[test]
    fn case_accents_and_punctuation_do_not_count() {
        let phrases = table(&BTreeMap::new());
        assert_eq!(command(&phrases, "Full  Width!").as_deref(), Some("dune action full_width"));
        assert_eq!(command(&phrases, "Next tab.").as_deref(), Some("dune action next_tab"));
        assert_eq!(command(&phrases, "Cénter").as_deref(), Some("dune action center"));
    }

    #[test]
    fn nothing_or_something_else_is_not_a_phrase() {
        let phrases = table(&BTreeMap::new());
        assert_eq!(find(&phrases, ""), None);
        assert_eq!(find(&phrases, "[unk]"), None);
        assert_eq!(find(&phrases, "desk"), None);
        assert_eq!(find(&phrases, "close [unk]"), None);
        assert_eq!(find(&phrases, "open"), None);
        assert_eq!(find(&phrases, "Yeah."), None);
    }

    #[test]
    fn free_text_goes_to_the_finder_quoted() {
        let phrases = table(&BTreeMap::new());
        assert_eq!(command(&phrases, "Open Firefox.").as_deref(), Some("dune find --pick apps 'firefox'"));
        assert_eq!(command(&phrases, "find visual studio code").as_deref(), Some("dune find apps 'visual studio code'"));
        assert_eq!(command(&phrases, "Go to Slack").as_deref(), Some("dune find --pick windows 'slack'"));
        assert_eq!(command(&phrases, "New tab, Firefox.").as_deref(), Some("dune find --pick apps 'firefox' :t"));
        assert_eq!(command(&phrases, "new tab").as_deref(), Some("dune action empty_tab"));
        assert_eq!(command(&phrases, "Next.").as_deref(), Some("dune action next_tab"));
        assert_eq!(command(&phrases, "back").as_deref(), Some("dune action prev_tab"));
        // The phrase with the most words around the text wins.
        assert_eq!(command(&phrases, "open file notes").as_deref(), Some("dune find --pick files 'notes'"));
        // A phrase without text first: "go to" would take it otherwise.
        let phrases = table(&file(&[("go to sleep", "systemctl suspend")]));
        assert_eq!(command(&phrases, "go to sleep").as_deref(), Some("systemctl suspend"));
        // What is said never leaves the quotes.
        let phrases = table(&file(&[("say {text}", "echo {text}")]));
        assert_eq!(command(&phrases, "say it's done; rm -rf").as_deref(), Some("echo 'its done rm rf'"));
    }

    #[test]
    fn a_program_or_a_terminal_opens_on_a_side() {
        let phrases = table(&BTreeMap::new());
        assert_eq!(command(&phrases, "Terminal left."), Some("dune action terminal_left".into()));
        assert_eq!(command(&phrases, "terminal right"), Some("dune action terminal_vertical".into()));
        assert_eq!(command(&phrases, "terminal top"), Some("dune action terminal_above".into()));
        assert_eq!(command(&phrases, "terminal below"), Some("dune action terminal_horizontal".into()));
        assert_eq!(command(&phrases, "Firefox left"), Some("dune find --pick apps 'firefox' :left".into()));
        assert_eq!(command(&phrases, "open firefox top"), Some("dune find --pick apps 'firefox' :up".into()));
        assert_eq!(command(&phrases, "visual studio code below"), Some("dune find --pick apps 'visual studio code' :down".into()));
        assert_eq!(command(&phrases, "move left"), Some("dune action swap_left".into()));
        assert_eq!(command(&phrases, "monitor right"), Some("dune action focus_monitor_right".into()));
        assert_eq!(command(&phrases, "monitor"), Some("dune action focus_monitor_next".into()));
        assert_eq!(command(&phrases, "send monitor"), Some("dune action move_monitor_next".into()));
        assert_eq!(command(&phrases, "click"), Some("dune pointer click".into()));
        assert_eq!(command(&phrases, "finder"), Some("dune action launcher".into()));
        assert_eq!(command(&phrases, "finder apps"), Some("dune find windows".into()));
        assert_eq!(command(&phrases, "finder files"), Some("dune find files".into()));
        assert_eq!(command(&phrases, "right click"), Some("dune pointer click right".into()));
        assert_eq!(command(&phrases, "open firefox"), Some("dune find --pick apps 'firefox'".into()));
    }

    #[test]
    fn text_can_sit_between_words() {
        let phrases = table(&file(&[("put {text} on the shelf", "echo {text}")]));
        assert_eq!(command(&phrases, "put the browser on the shelf").as_deref(), Some("echo 'the browser'"));
        assert_eq!(find(&phrases, "put on the shelf"), None);
    }

    #[test]
    fn the_file_adds_replaces_and_removes() {
        let phrases = table(&file(&[
            ("Settings", "qs -c dune ipc call settings toggle"),
            ("close", "dune action close_group"),
            ("lock", "none"),
            ("go to desk {n}", "dune action desk_{n}"),
        ]));
        assert_eq!(command(&phrases, "settings").as_deref(), Some("qs -c dune ipc call settings toggle"));
        assert_eq!(command(&phrases, "close").as_deref(), Some("dune action close_group"));
        assert_eq!(find(&phrases, "lock"), None);
        assert_eq!(command(&phrases, "go to desk two").as_deref(), Some("dune action desk_2"));
    }

    #[test]
    fn phrases_are_only_words_and_placeholders() {
        let phrases = table(&file(&[("say \"hi\", ok?", "echo hi"), ("Note {TEXT}", "echo {text}")]));
        assert!(phrases.iter().any(|p| p.said == "say hi ok"));
        assert!(phrases.iter().any(|p| p.said == "note {text}"));
        assert!(phrases.iter().filter(|p| !p.free()).all(|p| p.said.chars().all(|c| c.is_alphabetic() || c == ' ')));
    }
}
