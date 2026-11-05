//! Phrases that depend on what is on screen, tried before the phrase table:
//! - hints on screen (`dune-a11y hints`, `$XDG_RUNTIME_DIR/dune-hints.json`): what is heard
//!   is the word of the one to click, for `dune-a11y hint`;
//! - the minimap open (dune-shell writes what it shows to
//!   `$XDG_RUNTIME_DIR/dune-shell-minimap-OUTPUT.json`): "next", "back", "first", "last",
//!   "middle", or a window's name (the first one of that name) picks a window in it;
//!   "done" closes it;
//! - a Chromium browser focused (Vivaldi, Chrome, Brave…): "open x.com", "search …",
//!   "devtools", tabs, back and forward, through its keys (`wtype`), and "tab …" through
//!   its DevTools HTTP API (`--remote-debugging-port`, 9222 or `DUNE_VOICE_CDP_PORT`).
//!
//! Nothing of it heard: the phrase table has its turn, then the strip of tiles of the
//! focused monitor (`dune events window,strip`): "first", "last", or a program's name (the
//! first tile, left to right, of that app, else with those words in its title) goes to it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

use crate::phrases::{quote, words};

/// What a context does with a phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// A command for `sh -c`.
    Run(String),
    /// Show the browser tab whose title or address has these words.
    Tab(String),
}

/// A window the minimap shows.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MapTile {
    pub tile: u64,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub title: String,
}

/// What an open minimap shows, left to right.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Minimap {
    #[serde(default)]
    pub focused: Option<u64>,
    #[serde(default)]
    pub tiles: Vec<MapTile>,
}

/// The app id of the focused window, as `dune events window` last told it.
static FOCUS: Mutex<Option<String>> = Mutex::new(None);

/// The windows and strips, as `dune events window,strip` last told them.
static SCREEN: Mutex<Option<Screen>> = Mutex::new(None);

/// What dune shows: each window's app and title, each monitor's strip left to right.
#[derive(Debug, Default)]
pub struct Screen {
    focused: Option<u64>,
    windows: HashMap<u64, MapTile>,
    strips: HashMap<String, Vec<u64>>,
}

#[derive(Deserialize)]
struct Placed {
    tile: u64,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct ScreenEvent {
    event: String,
    #[serde(default)]
    change: String,
    #[serde(default)]
    tile: u64,
    #[serde(default)]
    app_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    output: String,
    #[serde(default)]
    tiles: Vec<Placed>,
}

impl Screen {
    /// A line of `dune events window,strip`.
    pub fn apply(&mut self, line: &str) {
        let Ok(e) = serde_json::from_str::<ScreenEvent>(line) else { return };
        match (e.event.as_str(), e.change.as_str()) {
            ("window", "close") => {
                self.windows.remove(&e.tile);
            }
            ("window", change) => {
                if change == "focus" {
                    self.focused = Some(e.tile);
                }
                self.windows.insert(e.tile, MapTile { tile: e.tile, app: e.app_id, title: e.title });
            }
            ("strip", _) => {
                let mut tiles = e.tiles;
                tiles.sort_by_key(|t| (t.x, t.y));
                self.strips.insert(e.output, tiles.into_iter().map(|t| t.tile).collect());
            }
            _ => {}
        }
    }

    /// The tiles of the focused window's strip, left to right.
    fn strip(&self) -> Vec<MapTile> {
        let focused = self.focused.and_then(|f| self.strips.values().find(|s| s.contains(&f)));
        let unknown = |tile: u64| MapTile { tile, app: String::new(), title: String::new() };
        focused
            .map(|s| s.iter().map(|t| self.windows.get(t).cloned().unwrap_or_else(|| unknown(*t))).collect())
            .unwrap_or_default()
    }
}

/// The phrase said, as a tile of the focused strip to go to: "first", "last", a program.
pub fn strip_command(heard: &str) -> Option<String> {
    let screen = SCREEN.lock().ok()?;
    strip_command_on(screen.as_ref()?, heard)
}

fn strip_command_on(screen: &Screen, heard: &str) -> Option<String> {
    let tiles = screen.strip();
    let said = words(heard);
    let tile = match said.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => return None,
        ["first"] => tiles.first()?,
        ["last"] => tiles.last()?,
        _ => {
            // An app id has its name anywhere: org.gnome.Terminal, org.wezfurlong.wezterm.
            let app_has = |t: &MapTile| words(&t.app.replace(['-', '_', '.'], " ")).windows(said.len()).any(|w| w == said);
            let i = tiles.iter().position(app_has).or_else(|| named(&tiles, &said))?;
            &tiles[i]
        }
    };
    Some(format!("dune goto {}", tile.tile))
}

/// "dune …": the rest, to go past what is on screen to the phrase table ("dune next" is
/// dune's next tab with a browser focused). As Hex wrote it, punctuation kept.
pub fn past_screen(heard: &str) -> Option<&str> {
    let (first, rest) = heard.trim().split_once(char::is_whitespace)?;
    (words(first) == ["dune"] && !words(rest).is_empty()).then(|| rest.trim_start())
}

/// What to do with `heard`, given the screen: the minimap first, then the focused browser.
pub fn command(heard: &str) -> Option<Do> {
    if hints_open() {
        return Some(Do::Run(format!("dune-a11y hint {}", quote(heard))));
    }
    if let Some(map) = open_minimap()
        && let Some(command) = minimap_command(&map, heard)
    {
        return Some(Do::Run(command));
    }
    let app = FOCUS.lock().ok()?.clone()?;
    if is_browser(&app) {
        return browser_command(heard);
    }
    None
}

/// How long hints stay, in seconds (dune-a11y's OPEN_FOR).
const HINTS_FOR: f64 = 15.0;

/// What `dune-a11y hints` wrote: when, and whether any are left.
#[derive(Debug, Default, Deserialize)]
struct Hints {
    #[serde(default)]
    at: f64,
    #[serde(default)]
    hints: Vec<serde_json::Value>,
}

/// Whether hints wrote at `at` are still shown at `now`.
fn hints_shown(text: &str, now: f64) -> bool {
    serde_json::from_str::<Hints>(text).is_ok_and(|h| !h.hints.is_empty() && now - h.at <= HINTS_FOR)
}

fn hints_open() -> bool {
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else { return false };
    let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join("dune-hints.json")) else { return false };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    hints_shown(&text, now)
}

/// The minimap open on some monitor, if one is (with windows in it).
fn open_minimap() -> Option<Minimap> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("dune-shell-minimap-"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<Minimap>(&text).ok())
        .find(|map| !map.tiles.is_empty())
}

/// The phrase said with the minimap open: a window of it to go to, or closing it.
pub fn minimap_command(map: &Minimap, heard: &str) -> Option<String> {
    let heard = words(heard);
    let said: Vec<&str> = heard.iter().map(String::as_str).collect();
    let tiles = &map.tiles;
    let last = tiles.len().checked_sub(1)?;
    let at = map.focused.and_then(|f| tiles.iter().position(|t| t.tile == f));
    let index = match said.as_slice() {
        ["done"] | ["ok"] | ["okay"] | ["close", "map"] | ["map"] => {
            return Some("dune action minimap".into());
        }
        ["next"] | ["right"] => at.map_or(0, |i| (i + 1).min(last)),
        ["back"] | ["previous"] | ["left"] => at.map_or(0, |i| i.saturating_sub(1)),
        ["first"] => 0,
        ["last"] => last,
        ["middle"] | ["center"] => last / 2,
        [] => return None,
        _ => named(tiles, &heard)?,
    };
    Some(format!("dune goto {}", tiles[index].tile))
}

/// The first window whose app is what was said ("vivaldi" for vivaldi-stable), else the
/// first whose title has those words.
fn named(tiles: &[MapTile], said: &[String]) -> Option<usize> {
    let starts = |of: &str| {
        let of = words(&of.replace(['-', '_', '.'], " "));
        of.len() >= said.len() && of[..said.len()] == *said
    };
    let within = |of: &str| {
        let of = words(of);
        of.windows(said.len()).any(|w| w == said)
    };
    tiles.iter().position(|t| starts(&t.app)).or_else(|| tiles.iter().position(|t| within(&t.title)))
}

/// Chromium browsers: they share the keys and the DevTools API.
fn is_browser(app: &str) -> bool {
    let app = app.to_lowercase();
    ["vivaldi", "chrom", "brave", "edge", "opera"].iter().any(|name| app.contains(name))
}

/// The keys each browser phrase presses (`wtype` arguments).
/// Each `wtype` is a new virtual keyboard with its own keymap: `-s 50` gives the browser
/// time to take it before the first key, else it may read that key with the keymap of the
/// one before.
const BROWSER_KEYS: &[(&str, &str)] = &[
    ("devtools", "-M ctrl -M shift i -m shift -m ctrl"),
    ("dev tools", "-M ctrl -M shift i -m shift -m ctrl"),
    ("back", "-M alt -k Left -m alt"),
    ("forward", "-M alt -k Right -m alt"),
    ("reload", "-k F5"),
    ("refresh", "-k F5"),
    ("next", "-M ctrl -k Tab -m ctrl"),
    ("next tab", "-M ctrl -k Tab -m ctrl"),
    ("previous", "-M ctrl -M shift -k Tab -m shift -m ctrl"),
    ("previous tab", "-M ctrl -M shift -k Tab -m shift -m ctrl"),
    ("new tab", "-M ctrl t -m ctrl"),
    ("close tab", "-M ctrl w -m ctrl"),
    ("close up", "-M ctrl w -m ctrl"),
    // Hex writes "close up" so.
    ("closed up", "-M ctrl w -m ctrl"),
    ("reopen tab", "-M ctrl -M shift t -m shift -m ctrl"),
    ("address", "-M ctrl l -m ctrl"),
    ("top", "-k Home"),
    ("bottom", "-k End"),
    ("scroll down", "-k Page_Down"),
    ("page down", "-k Page_Down"),
    ("scroll up", "-k Page_Up"),
    ("page up", "-k Page_Up"),
    ("zoom in", "-M ctrl plus -m ctrl"),
    ("zoom out", "-M ctrl minus -m ctrl"),
    ("zoom reset", "-M ctrl 0 -m ctrl"),
    ("commands", QUICK_COMMANDS),
    ("quick commands", QUICK_COMMANDS),
];

/// Vivaldi's Quick Commands, on F2 (set in Vivaldi's keyboard settings): one key, no
/// modifier to get wrong. Ctrl+E sometimes scrolled the page instead. Pressed without the
/// `-s 50`.
const QUICK_COMMANDS: &str = "-k F2";

/// What can be said to a focused browser or the open minimap without free text, for
/// `dune-voice fix` to offer.
pub fn phrases() -> Vec<&'static str> {
    let mut out: Vec<&str> = BROWSER_KEYS.iter().map(|(p, _)| *p).collect();
    out.extend(["done", "first", "last", "middle", "next", "back", "previous"]);
    out
}

/// The phrase said to a focused browser.
pub fn browser_command(heard: &str) -> Option<Do> {
    let said = words(heard).join(" ");
    if let Some((_, keys)) = BROWSER_KEYS.iter().find(|(phrase, _)| *phrase == said) {
        let pause = if *keys == QUICK_COMMANDS { "" } else { "-s 50 " };
        return Some(Do::Run(format!("wtype {pause}{keys}")));
    }
    let after = |word: &str| said.strip_prefix(word).and_then(|rest| rest.strip_prefix(' ')).map(str::to_string);
    // "open x.com", "go to github dot com": that address, in this tab. Not an address
    // ("open firefox"): the phrase table's (an app).
    if let Some(text) = after("open").or_else(|| after("go to")) {
        let address = address(heard, &text)?;
        return Some(Do::Run(typed(&["-M ctrl l -m ctrl"], &address)));
    }
    // A new tab, the text in its address bar: the browser's own search engine.
    if let Some(text) = after("search") {
        return Some(Do::Run(typed(&["-M ctrl t -m ctrl"], &text)));
    }
    // "command bookmarks": Quick Commands, that text, Enter.
    if let Some(text) = after("command").or_else(|| after("quick command")) {
        return Some(Do::Run(format!("wtype {QUICK_COMMANDS} && sleep 0.15 && wtype -- {} && wtype -k Return", quote(&text))));
    }
    if let Some(text) = after("find in page").or_else(|| after("find on page")) {
        return Some(Do::Run(format!("wtype -s 50 -M ctrl f -m ctrl && sleep 0.15 && wtype -s 50 -- {}", quote(&text))));
    }
    if let Some(text) = after("tab") {
        return Some(Do::Tab(text));
    }
    None
}

/// Keys, a breath for the browser to show the address bar, the text, then Enter.
fn typed(keys: &[&str], text: &str) -> String {
    let keys: Vec<String> = keys.iter().map(|k| format!("wtype -s 50 {k}")).collect();
    format!("{} && sleep 0.15 && wtype -s 50 -- {} && wtype -s 50 -k Return", keys.join(" && "), quote(text))
}

/// The address said after "open": as Hex wrote it ("x.com") or spelled out ("x dot com"),
/// lowercase, no spaces. Only when it has a dot and a name around it.
fn address(heard: &str, said: &str) -> Option<String> {
    // Hex's own punctuation is kept for the dots ("X.com"); `said` lost it.
    let raw = heard.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    let raw = raw.split_once(' ').map_or(raw.as_str(), |(_, rest)| rest);
    let raw = raw.strip_prefix("to ").unwrap_or(raw);
    let text = if raw.contains('.') { raw.to_string() } else { said.replace(" dot ", ".") };
    let address: String = text.split_whitespace().collect();
    let (name, domain) = address.rsplit_once('.')?;
    (!name.is_empty() && domain.len() >= 2 && domain.chars().all(|c| c.is_ascii_alphabetic())).then_some(address)
}

/// The DevTools port of the browser.
fn cdp_port() -> u16 {
    std::env::var("DUNE_VOICE_CDP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(9222)
}

/// One GET to the browser's DevTools HTTP API: the body.
fn cdp_get(path: &str) -> Result<String, String> {
    let port = cdp_port();
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("the browser's DevTools are not on port {port} ({e})"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    write!(stream, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n").map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream).read_to_string(&mut reply).map_err(|e| e.to_string())?;
    let (_, body) = reply.split_once("\r\n\r\n").ok_or("the DevTools reply has no body")?;
    Ok(body.to_string())
}

#[derive(Debug, Deserialize)]
struct Target {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
}

/// Shows the first page whose title or address has the words said: its title.
pub fn show_tab(said: &str) -> Result<String, String> {
    let list = cdp_get("/json/list")?;
    let targets: Vec<Target> = serde_json::from_str(&list).map_err(|e| e.to_string())?;
    let id = pick_tab(&targets, said).ok_or_else(|| format!("no tab with “{said}”"))?;
    let target = targets.iter().find(|t| t.id == id).unwrap();
    cdp_get(&format!("/json/activate/{}", target.id))?;
    Ok(target.title.clone())
}

fn pick_tab(targets: &[Target], said: &str) -> Option<String> {
    let said = words(said);
    let has = |text: &str| {
        let of = words(&text.replace(['-', '_', '.', '/', ':'], " "));
        !said.is_empty() && of.windows(said.len()).any(|w| w == said)
    };
    let pages = || targets.iter().filter(|t| t.kind == "page");
    pages().find(|t| has(&t.title)).or_else(|| pages().find(|t| has(&t.url))).map(|t| t.id.clone())
}

/// Keeps the app of the focused window and the strips, for as long as dune-voice runs
/// (`dune events window,strip` again whenever it ends).
pub fn follow_focus(retry: Duration) {
    loop {
        if let Ok(mut child) = Command::new("dune")
            .args(["events", "window,strip"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Ok(mut screen) = SCREEN.lock() {
                *screen = Some(Screen::default());
            }
            let lines = child.stdout.take().map(|out| BufReader::new(out).lines());
            for line in lines.into_iter().flatten().map_while(Result::ok) {
                if let Some(app) = focused_app(&line)
                    && let Ok(mut focus) = FOCUS.lock()
                {
                    *focus = Some(app);
                }
                if let Ok(mut screen) = SCREEN.lock()
                    && let Some(screen) = screen.as_mut()
                {
                    screen.apply(&line);
                }
            }
            let _ = child.wait();
        }
        if let Ok(mut focus) = FOCUS.lock() {
            *focus = None;
        }
        if let Ok(mut screen) = SCREEN.lock() {
            *screen = None;
        }
        std::thread::sleep(retry);
    }
}

#[derive(Deserialize)]
struct WindowEvent {
    event: String,
    change: String,
    #[serde(default)]
    app_id: String,
}

/// The app of a window `focus` line of `dune events`.
fn focused_app(line: &str) -> Option<String> {
    let event: WindowEvent = serde_json::from_str(line).ok()?;
    (event.event == "window" && event.change == "focus").then_some(event.app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_are_shown_while_their_time_lasts() {
        let text = r#"{"output":"HDMI-A-1","at":100.5,"hints":[{"word":"apple"}]}"#;
        assert!(hints_shown(text, 110.0));
        assert!(!hints_shown(text, 116.0));
        assert!(!hints_shown("{}", 100.0));
        assert!(!hints_shown("not json", 100.0));
    }

    #[test]
    fn dune_goes_past_the_screen() {
        assert_eq!(past_screen("Dune, next."), Some("next."));
        assert_eq!(past_screen("dune new tab firefox"), Some("new tab firefox"));
        assert_eq!(past_screen("dune"), None);
        assert_eq!(past_screen("Dune."), None);
        assert_eq!(past_screen("next"), None);
        assert_eq!(past_screen("dunes next"), None);
    }

    fn map() -> Minimap {
        let tile = |tile, app: &str, title: &str| MapTile { tile, app: app.into(), title: title.into() };
        Minimap {
            focused: Some(2),
            tiles: vec![
                tile(1, "Terminal", "~/dune"),
                tile(2, "vivaldi-stable", "GitHub - Vivaldi"),
                tile(3, "Terminal", "vim notes"),
                tile(4, "firefox", "News"),
            ],
        }
    }

    #[test]
    fn the_minimap_moves_from_the_focused_window() {
        let map = map();
        let go = |heard| minimap_command(&map, heard);
        assert_eq!(go("Next."), Some("dune goto 3".into()));
        assert_eq!(go("back"), Some("dune goto 1".into()));
        assert_eq!(go("First"), Some("dune goto 1".into()));
        assert_eq!(go("last"), Some("dune goto 4".into()));
        assert_eq!(go("middle"), Some("dune goto 2".into()));
        assert_eq!(go("done"), Some("dune action minimap".into()));
        let edge = Minimap { focused: Some(4), ..map.clone() };
        assert_eq!(minimap_command(&edge, "next"), Some("dune goto 4".into()));
    }

    #[test]
    fn the_minimap_goes_to_the_first_window_of_a_name() {
        let map = map();
        assert_eq!(minimap_command(&map, "terminal"), Some("dune goto 1".into()));
        assert_eq!(minimap_command(&map, "Vivaldi"), Some("dune goto 2".into()));
        assert_eq!(minimap_command(&map, "notes"), Some("dune goto 3".into()));
        assert_eq!(minimap_command(&map, "desk two"), None);
    }

    #[test]
    fn the_browser_opens_addresses_and_searches() {
        let run = |cmd: &str| Some(Do::Run(cmd.into()));
        assert_eq!(
            browser_command("Open X.com."),
            run("wtype -s 50 -M ctrl l -m ctrl && sleep 0.15 && wtype -s 50 -- 'x.com' && wtype -s 50 -k Return")
        );
        assert_eq!(
            browser_command("open github dot com"),
            run("wtype -s 50 -M ctrl l -m ctrl && sleep 0.15 && wtype -s 50 -- 'github.com' && wtype -s 50 -k Return")
        );
        assert_eq!(browser_command("open firefox"), None);
        assert_eq!(
            browser_command("Search rust borrow checker"),
            run("wtype -s 50 -M ctrl t -m ctrl && sleep 0.15 && wtype -s 50 -- 'rust borrow checker' && wtype -s 50 -k Return")
        );
        assert_eq!(browser_command("Closed up."), run("wtype -s 50 -M ctrl w -m ctrl"));
        assert_eq!(browser_command("close up"), run("wtype -s 50 -M ctrl w -m ctrl"));
        assert_eq!(browser_command("DevTools"), run("wtype -s 50 -M ctrl -M shift i -m shift -m ctrl"));
        assert_eq!(browser_command("back"), run("wtype -s 50 -M alt -k Left -m alt"));
        assert_eq!(browser_command("tab github"), Some(Do::Tab("github".into())));
        assert_eq!(browser_command("Commands."), run("wtype -k F2"));
        assert_eq!(browser_command("quick commands"), run("wtype -k F2"));
        assert_eq!(
            browser_command("command bookmarks"),
            run("wtype -k F2 && sleep 0.15 && wtype -- 'bookmarks' && wtype -k Return")
        );
        assert_eq!(browser_command("desk two"), None);
    }

    #[test]
    fn tabs_by_title_then_address() {
        let target = |id: &str, kind: &str, title: &str, url: &str| Target {
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            url: url.into(),
        };
        let targets = vec![
            target("w", "service_worker", "GitHub", "https://github.com/sw.js"),
            target("a", "page", "Inbox - Mail", "https://mail.example.com"),
            target("b", "page", "Home / X", "https://x.com/home"),
            target("c", "page", "dune: GitHub repo", "https://github.com/dune"),
        ];
        assert_eq!(pick_tab(&targets, "github"), Some("c".into()));
        assert_eq!(pick_tab(&targets, "x com"), Some("b".into()));
        assert_eq!(pick_tab(&targets, "calendar"), None);
    }

    #[test]
    fn focus_lines() {
        let line = r#"{"event":"window","change":"focus","tile":3,"app_id":"vivaldi-stable","title":"x","place":"desk 1"}"#;
        assert_eq!(focused_app(line), Some("vivaldi-stable".into()));
        assert_eq!(focused_app(&line.replace("focus", "title")), None);
        assert!(is_browser("vivaldi-stable") && !is_browser("foot"));
    }

    #[test]
    fn the_strip_goes_first_last_and_by_program() {
        let mut screen = Screen::default();
        for line in [
            r#"{"event":"window","change":"new","tile":26,"app_id":"org.wezfurlong.wezterm","title":"~"}"#,
            r#"{"event":"window","change":"new","tile":2,"app_id":"vivaldi-stable","title":"X - Vivaldi"}"#,
            r#"{"event":"window","change":"new","tile":18,"app_id":"org.gnome.Terminal","title":"me@hx90:~"}"#,
            r#"{"event":"window","change":"new","tile":12,"app_id":"firefox","title":"Square1"}"#,
            r#"{"event":"window","change":"new","tile":14,"app_id":"firefox","title":"other monitor"}"#,
            r#"{"event":"strip","change":"layout","output":"HDMI-A-1","view":0,"tiles":[{"tile":12,"x":2561,"y":7},{"tile":26,"x":7,"y":31},{"tile":2,"x":1284,"y":31},{"tile":18,"x":3838,"y":7}]}"#,
            r#"{"event":"strip","change":"layout","output":"HDMI-A-2","view":0,"tiles":[{"tile":14,"x":7,"y":7}]}"#,
            r#"{"event":"window","change":"focus","tile":2,"app_id":"vivaldi-stable","title":"X - Vivaldi"}"#,
        ] {
            screen.apply(line);
        }
        let go = |screen: &Screen, heard| strip_command_on(screen, heard);
        assert_eq!(go(&screen, "first"), Some("dune goto 26".into()));
        assert_eq!(go(&screen, "Last"), Some("dune goto 18".into()));
        assert_eq!(go(&screen, "firefox"), Some("dune goto 12".into()));
        assert_eq!(go(&screen, "wezterm"), Some("dune goto 26".into()));
        assert_eq!(go(&screen, "terminal"), Some("dune goto 18".into()));
        assert_eq!(go(&screen, "square one"), None);
        assert_eq!(go(&screen, "gedit"), None);
        screen.apply(r#"{"event":"window","change":"close","tile":18,"app_id":"org.gnome.Terminal","title":""}"#);
        assert_eq!(go(&screen, "terminal"), None);
    }
}
