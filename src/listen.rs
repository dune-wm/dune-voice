//! The daemon: it listens for one phrase, then runs its command, each time it is asked. Who
//! hears is `engine`: the running Hex (`hex dictation start`, then `stop --print`: the words,
//! never pasted), or Vosk, whose model (slow to load) it holds, on `pw-record`'s sound.
//! - Mod held alone (dune's `mod` events, `dune events mod`): it listens while Mod is down
//!   and takes the phrase when it is let go; another key meanwhile (a shortcut): nothing.
//! - `dune-voice listen`: it listens until a short silence after a phrase, or `timeout`;
//!   asked again meanwhile, it stops.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use vosk::{DecodingState, LogLevel, Model, Recognizer};

use crate::config::{self, Config, Engine};
use crate::context::{self, Do};
use crate::learned::Learned;
use crate::phrases;

/// Vosk models take 16 kHz mono 16-bit sound.
const RATE: u32 = 16_000;
/// Sound handed to Vosk at a time: 100 ms.
const CHUNK: usize = RATE as usize / 10;
/// The longest Mod can be held for one phrase.
const HELD_AT_MOST: Duration = Duration::from_secs(15);
/// How long to wait before following dune's events again when they end (dune restarting).
const RETRY: Duration = Duration::from_secs(3);

/// Where `dune-voice listen` finds the daemon.
pub fn socket_path() -> Result<PathBuf, String> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(|dir| PathBuf::from(dir).join("dune-voice.sock"))
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set".to_string())
}

/// `dune-voice listen`: have the daemon listen for a phrase (listening: stop).
pub fn ask_to_listen() -> Result<(), String> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("dune-voice is not running ({}: {e})", path.display()))?;
    stream.write_all(b"listen").map_err(|e| e.to_string())
}

/// How a listening goes, shared with what asked for it.
#[derive(Default)]
struct Listening {
    /// One is going on.
    on: AtomicBool,
    /// Stop and take what was said (Mod let go, `listen` again).
    stop: AtomicBool,
    /// Stop and do nothing (Mod was part of a shortcut).
    drop: AtomicBool,
    /// `dune-voice train` going: what is heard goes to it, nothing runs.
    training: Mutex<Option<Training>>,
}

/// `dune-voice train PHRASE`: the next `times` phrases heard are told to it (`heard\tTEXT`
/// lines, then `done`), not run.
struct Training {
    to: UnixStream,
    phrase: String,
    times: usize,
    told: usize,
}

/// `dune-voice train`: hear `phrase` `times` times, holding Mod; what was heard each time.
/// Ends early (what was heard so far) when the daemon stops or another `train` starts.
pub fn train(phrase: &str, times: usize, mut each: impl FnMut(usize, &str)) -> Result<Vec<String>, String> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("dune-voice is not running ({}: {e})", path.display()))?;
    stream.write_all(format!("train\t{phrase}\t{times}").as_bytes()).map_err(|e| e.to_string())?;
    stream.shutdown(std::net::Shutdown::Write).map_err(|e| e.to_string())?;
    let mut heard = Vec::new();
    for line in BufReader::new(stream).lines() {
        let line = line.map_err(|e| e.to_string())?;
        match line.split_once('\t') {
            Some(("heard", text)) => {
                heard.push(text.to_string());
                each(heard.len(), text);
            }
            Some(("error", why)) => return Err(why.to_string()),
            _ if line == "done" => break,
            _ => {}
        }
    }
    Ok(heard)
}

/// How a listening ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Until {
    /// `stop` (Mod let go), at most `HELD_AT_MOST`.
    Stopped,
    /// A phrase then a short silence, `stop` or `timeout`.
    Silence,
}

/// What hears, chosen when dune-voice starts.
#[derive(Clone)]
enum Ear {
    Hex,
    Vosk(Arc<Model>),
}

/// `dune-voice`: load the model (Vosk), then listen each time asked, until killed or another
/// `dune-voice` starts (each dune session starts its own).
pub fn serve() -> Result<(), String> {
    let config = config::load()?;
    let ear = match config.engine {
        Engine::Hex => Ear::Hex,
        Engine::Vosk => {
            vosk::set_log_level(LogLevel::Error);
            let mut model = Model::new(config.model.to_string_lossy())
                .ok_or_else(|| format!("no Vosk model in {} (see the README)", config.model.display()))?;
            for word in unknown_words(&mut model, &config) {
                eprintln!("dune-voice: the model does not know `{word}`: phrases with it cannot be heard");
            }
            Ear::Vosk(Arc::new(model))
        }
    };

    let path = socket_path()?;
    // One already running (maybe of an older dune session, following its dune): it goes.
    if let Ok(mut old) = UnixStream::connect(&path) {
        let _ = old.write_all(b"quit");
        drop(old);
        std::thread::sleep(Duration::from_millis(300));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let engine = if matches!(ear, Ear::Hex) { "hex" } else { "vosk" };
    eprintln!("dune-voice: ready, {} phrases, {engine} ({})", config.phrases.len(), path.display());

    // English by default: the phrases are (a language set for dictation goes back to it).
    if matches!(ear, Ear::Hex) {
        std::thread::spawn(|| {
            for _ in 0..20 {
                match language("en", false) {
                    Err(e) if e.contains("not running") => std::thread::sleep(RETRY),
                    Err(e) => return eprintln!("dune-voice: {e}"),
                    Ok(()) => return,
                }
            }
        });
    }
    let listening = Arc::new(Listening::default());
    {
        let (ear, listening) = (ear.clone(), listening.clone());
        std::thread::spawn(move || follow_mod(&ear, &listening));
    }
    // The focused window, for the phrases of a browser.
    std::thread::spawn(|| context::follow_focus(RETRY));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut asked = String::new();
        let _ = stream.read_to_string(&mut asked);
        match asked.trim() {
            "listen" => {}
            "quit" => break,
            asked if asked.starts_with("train\t") => {
                begin_training(asked, stream, matches!(ear, Ear::Vosk(_)), config.notify, &listening);
                continue;
            }
            _ => continue,
        }
        if listening.on.load(Ordering::SeqCst) {
            listening.stop.store(true, Ordering::SeqCst);
        } else {
            start(&ear, &listening, Until::Silence);
        }
    }
    Ok(())
}

/// A `train\tPHRASE\tTIMES` request: the next phrases heard go to it (one going before:
/// it ends, told nothing more).
fn begin_training(asked: &str, mut to: UnixStream, vosk: bool, notify: bool, listening: &Listening) {
    let mut parts = asked.splitn(3, '\t').skip(1);
    let phrase = parts.next().unwrap_or("").trim().to_string();
    let times = parts.next().and_then(|n| n.trim().parse().ok()).unwrap_or(10usize).clamp(1, 50);
    let _ = to.set_write_timeout(Some(Duration::from_secs(1)));
    if vosk {
        let _ = to.write_all(b"error\tVosk only hears the phrases themselves: train is for Hex\n");
        return;
    }
    if phrase.is_empty() {
        let _ = to.write_all(b"error\tno phrase to train\n");
        return;
    }
    Notice::new(notify).show(&format!("Training «{phrase}»"), &format!("hold Mod and say it: 0/{times}"), Duration::from_secs(5));
    *listening.training.lock().unwrap_or_else(|e| e.into_inner()) = Some(Training { to, phrase, times, told: 0 });
}

/// What was heard told to the `train` going, if one is: whether one was.
fn tell_training(listening: &Listening, text: &str, notice: &mut Notice) -> bool {
    let mut training = listening.training.lock().unwrap_or_else(|e| e.into_inner());
    let Some(t) = training.as_mut() else { return false };
    if text.is_empty() {
        notice.say("Heard nothing", &format!("say «{}» again: {}/{}", t.phrase, t.told, t.times));
        return true;
    }
    if t.to.write_all(format!("heard\t{}\n", text.replace(['\t', '\n'], " ")).as_bytes()).is_err() {
        *training = None; // `train` gone (Ctrl+C): this phrase runs, as all after it
        return false;
    }
    t.told += 1;
    if t.told >= t.times {
        let _ = t.to.write_all(b"done\n");
        notice.say(&format!("Training «{}» done", t.phrase), &format!("last heard: {text}"));
        *training = None;
    } else {
        notice.say(&format!("{}/{}: {text}", t.told, t.times), &format!("again: «{}»", t.phrase));
    }
    true
}

/// Listen in a thread of its own, unless already listening.
fn start(ear: &Ear, listening: &Arc<Listening>, until: Until) {
    if listening.on.swap(true, Ordering::SeqCst) {
        return;
    }
    listening.stop.store(false, Ordering::SeqCst);
    listening.drop.store(false, Ordering::SeqCst);
    let (ear, listening) = (ear.clone(), listening.clone());
    std::thread::spawn(move || {
        hear(&ear, &listening, until);
        listening.on.store(false, Ordering::SeqCst);
    });
}

/// dune's `mod` events, for as long as dune-voice runs: `dune events mod` again whenever
/// it ends (dune restarting, or not up yet).
fn follow_mod(ear: &Ear, listening: &Arc<Listening>) {
    let mut told = false;
    loop {
        match Command::new("dune").args(["events", "mod"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(mut child) => {
                let lines = child.stdout.take().map(|out| BufReader::new(out).lines());
                let mut dictation = Dictation::default();
                for line in lines.into_iter().flatten().map_while(Result::ok) {
                    match mod_change(&line) {
                        Some((change, "shift")) => dictation.change(change),
                        Some(("start", _)) => start(ear, listening, Until::Stopped),
                        Some(("end", _)) => listening.stop.store(true, Ordering::SeqCst),
                        Some(("cancel", _)) => listening.drop.store(true, Ordering::SeqCst),
                        _ => {}
                    }
                }
                let mut why = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut why);
                }
                let _ = child.wait();
                // Said once: an older dune without `mod` events, or no dune, says it every time.
                if !why.trim().is_empty() && !told {
                    eprintln!("dune-voice: dune events mod: {} (Mod held alone will not listen)", why.trim());
                    told = true;
                }
            }
            Err(e) if !told => {
                eprintln!("dune-voice: dune events mod: {e}");
                told = true;
            }
            Err(_) => {}
        }
        std::thread::sleep(RETRY);
    }
}

/// The `change` of a `mod` line of `dune events`, and the `keys` held (`mod` from a dune
/// that does not say).
fn mod_change(line: &str) -> Option<(&'static str, &'static str)> {
    let line = line.replace(' ', "");
    if !line.contains(r#""event":"mod""#) {
        return None;
    }
    let change = ["start", "end", "cancel"].into_iter().find(|change| line.contains(&format!(r#""change":"{change}""#)))?;
    let keys = if line.contains(r#""keys":"shift""#) { "shift" } else { "mod" };
    Some((change, keys))
}

/// Shift held alone: Hex dictates into the focused app (`hex dictation start`); let go, it
/// pastes (`stop`); a key or a button meanwhile (a capital letter), it is dropped
/// (`stop --print`, printed to nobody). Only a dictation it started: one going on already
/// (Mod+Shift+., the Hex window) is left alone.
#[derive(Default)]
struct Dictation {
    on: bool,
}

impl Dictation {
    fn change(&mut self, change: &str) {
        match change {
            "start" if !self.on && !hex_busy() => {
                self.on = hex(&["dictation", "start"]);
            }
            "end" if self.on => {
                self.on = false;
                hex(&["dictation", "stop"]);
            }
            "cancel" if self.on => {
                self.on = false;
                hex(&["dictation", "stop", "--print"]);
            }
            _ => {}
        }
    }
}

/// Run `hex` with `args`; whether it did it.
fn hex(args: &[&str]) -> bool {
    match Command::new("hex").args(args).stdin(Stdio::null()).stdout(Stdio::null()).output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            eprintln!("dune-voice: hex {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
            false
        }
        Err(e) => {
            eprintln!("dune-voice: hex: {e}");
            false
        }
    }
}

/// The words of the phrases the model does not have (it would never hear them); those with
/// free text it never hears anyway.
pub fn unknown_words(model: &mut Model, config: &Config) -> Vec<String> {
    let mut words: Vec<String> =
        config.phrases.iter().filter(|p| !p.free()).flat_map(|p| p.said.split(' ').map(str::to_string)).collect();
    words.sort();
    words.dedup();
    words.retain(|word| model.find_word(word).is_none());
    words
}

/// Listen for one phrase, then run it.
fn hear(ear: &Ear, listening: &Listening, until: Until) {
    // Read again each time, so a change to the phrases applies on the next one.
    let config = match config::load() {
        Ok(config) => config,
        Err(e) => {
            Notice::new(true).say("dune-voice", &e);
            return;
        }
    };
    // A dictation of the user's own (`hex dictation toggle`, Mod+Shift+.) is Hex's already:
    // starting ours would change nothing, and stopping it would take its words.
    if matches!(ear, Ear::Hex) && hex_busy() {
        return;
    }
    let mut notice = Notice::new(config.notify);
    let time = if until == Until::Stopped { HELD_AT_MOST } else { config.timeout };
    let heard = match ear {
        Ear::Hex => hex_hear(time, listening, &mut notice),
        Ear::Vosk(model) => vosk_hear(model, &config, time, until, listening, &mut notice),
    };
    let heard = match heard {
        Ok(heard) => heard,
        Err(e) => return notice.say("dune-voice", &e),
    };

    if listening.drop.load(Ordering::SeqCst) {
        notice.show("Cancelled", "", Duration::from_millis(600));
        return;
    }
    let text = heard.trim();
    if tell_training(listening, text, &mut notice) {
        return;
    }
    // How Hex hears a phrase, learnt (`dune-voice train`): that phrase.
    let learned = Learned::load();
    let text = learned.apply(text);
    // What is on screen first: the minimap open, the browser focused.
    match context::command(text) {
        Some(Do::Run(command)) => {
            notice.say(&format!("✓ {text}"), "");
            return run(&command);
        }
        Some(Do::Tab(said)) => match context::show_tab(&said) {
            Ok(title) => return notice.say(&format!("✓ {text}"), &title),
            Err(e) => return notice.say("No tab", &e),
        },
        None => {}
    }
    // A phrase, else a tile of the strip ("first", "last", a program).
    let command = phrases::find_app(&config.phrases, text).map(|heard| heard.command.clone()).or_else(|| context::strip_command(text));
    match command {
        Some(command) => {
            notice.say(&format!("✓ {text}"), "");
            run(&command);
        }
        None if !text.is_empty() && text != "[unk]" => notice.say("Not understood", text),
        None if until == Until::Silence && listening.stop.load(Ordering::SeqCst) => notice.say("Stopped listening", ""),
        None => notice.say("Heard nothing", ""),
    }
}

/// `dune-voice language CODE`: Hex's model in that language (`hex model use`), unless it
/// is in it already (a switch takes 1.5–3 s); told in a notification when `tell`.
pub fn language(code: &str, tell: bool) -> Result<(), String> {
    let out = Command::new("hex").args(["status", "--json"]).stdin(Stdio::null()).output().map_err(|e| format!("hex: {e}"))?;
    if out.status.code() == Some(3) {
        return Err("Hex is not running (`hex start`)".into());
    }
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "hex status: not JSON".to_string())?;
    let (Some(model), now) = (status["model"]["id"].as_str(), status["model"]["language"].as_str()) else {
        return Err("hex status: no model".into());
    };
    let name = language_name(code);
    if now != Some(code) {
        let out = Command::new("hex")
            .args(["model", "use", model, "--language", code])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("hex: {e}"))?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Notice::new(tell).say("Dictation language not changed", &why);
            return Err(format!("hex model use {model} --language {code}: {why}"));
        }
    }
    Notice::new(tell).say(&format!("Dictation: {name}"), if code == "en" { "" } else { "say “dictate english” for Mod's phrases" });
    Ok(())
}

/// A language code's name, for the notification.
fn language_name(code: &str) -> String {
    match code {
        "en" => "English".into(),
        "es" => "Español".into(),
        "it" => "Italiano".into(),
        "fr" => "Français".into(),
        "de" => "Deutsch".into(),
        "pt" => "Português".into(),
        other => other.to_string(),
    }
}

/// Hex is recording or transcribing a dictation (one dune-voice did not start).
fn hex_busy() -> bool {
    Command::new("hex")
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| capture_busy(&String::from_utf8_lossy(&out.stdout)))
}

/// `capture.state` of `hex status --json` is something other than `idle`.
fn capture_busy(status: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(status)
        .ok()
        .and_then(|v| v["capture"]["state"].as_str().map(|s| s != "idle"))
        .unwrap_or(false)
}

/// What Hex heard, from `hex dictation start` until the listening ends (`time` at most):
/// `hex dictation stop --print` prints it, nothing pasted. Cancelled, Hex stops all the same.
fn hex_hear(time: Duration, listening: &Listening, notice: &mut Notice) -> Result<String, String> {
    let started = Command::new("hex").args(["dictation", "start", "--raw"]).stdin(Stdio::null()).output();
    match started {
        Ok(out) if out.status.success() => {}
        Ok(out) if out.status.code() == Some(3) => return Err("Hex is not running (`hex start`)".into()),
        Ok(out) => return Err(format!("hex dictation start: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(e) => return Err(format!("hex: {e}")),
    }
    notice.show("Listening…", "", HELD_AT_MOST);
    let start = Instant::now();
    while start.elapsed() < time && !listening.stop.load(Ordering::SeqCst) && !listening.drop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(30));
    }
    let out = Command::new("hex")
        .args(["dictation", "stop", "--print"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("hex: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()),
        // Nothing was said.
        Some(4) => Ok(String::new()),
        _ => Err(format!("hex dictation stop: {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
}

/// What Vosk heard of the phrases it is given (none with free text), on `pw-record`'s sound.
fn vosk_hear(
    model: &Model,
    config: &Config,
    time: Duration,
    until: Until,
    listening: &Listening,
    notice: &mut Notice,
) -> Result<String, String> {
    let mut grammar: Vec<&str> = config.phrases.iter().filter(|p| !p.free()).map(|p| p.said.as_str()).collect();
    // What is none of them is heard as this, not as the closest phrase.
    grammar.push("[unk]");
    let mut recognizer =
        Recognizer::new_with_grammar(model, RATE as f32, &grammar).ok_or("Vosk did not take the phrases")?;
    let mut record = Command::new("pw-record")
        .args(["--rate", &RATE.to_string(), "--channels", "1", "--format", "s16", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("pw-record: {e}"))?;
    notice.show("Listening…", "", HELD_AT_MOST);
    let heard = listen(&mut record, &mut recognizer, time, until, listening);
    let _ = record.kill();
    let _ = record.wait();
    Ok(heard.unwrap_or_default())
}

/// What was said, when the listening ends (`until`, or `time` is up); `None` when the
/// sound cannot be read.
fn listen(record: &mut Child, recognizer: &mut Recognizer, time: Duration, until: Until, listening: &Listening) -> Option<String> {
    let mut sound = record.stdout.take()?;
    let mut bytes = vec![0u8; CHUNK * 2];
    let mut samples = vec![0i16; CHUNK];
    // A phrase Vosk already closed (a pause after it), while Mod is still held.
    let mut said = String::new();
    let start = Instant::now();
    let ended = || listening.stop.load(Ordering::SeqCst) || listening.drop.load(Ordering::SeqCst);
    while start.elapsed() < time && !ended() {
        sound.read_exact(&mut bytes).ok()?;
        for (sample, pair) in samples.iter_mut().zip(bytes.chunks_exact(2)) {
            *sample = i16::from_le_bytes([pair[0], pair[1]]);
        }
        if let Ok(DecodingState::Finalized) = recognizer.accept_waveform(&samples) {
            let text = recognizer.result().single().map(|r| r.text.to_string()).unwrap_or_default();
            // Silence before anything was said: keep listening.
            if !text.trim().is_empty() && text.trim() != "[unk]" {
                if until == Until::Silence {
                    return Some(text);
                }
                said = text;
            }
        }
    }
    let rest = recognizer.final_result().single().map(|r| r.text.to_string()).unwrap_or_default();
    Some(if rest.trim().is_empty() { said } else { rest })
}

/// A phrase's command, through the shell, from the home folder; not waited for, but reaped.
fn run(command: &str) {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/".into());
    match Command::new("sh").arg("-c").arg(command).current_dir(home).stdin(Stdio::null()).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("dune-voice: {command}: {e}"),
    }
}

/// One notification, replaced as listening goes (notify-send's id), when `notify` is on.
struct Notice {
    on: bool,
    id: Option<String>,
}

impl Notice {
    fn new(on: bool) -> Notice {
        Notice { on, id: None }
    }

    /// Shown for 3 s.
    fn say(&mut self, summary: &str, body: &str) {
        self.show(summary, body, Duration::from_secs(3));
    }

    fn show(&mut self, summary: &str, body: &str, time: Duration) {
        if !self.on {
            return;
        }
        let mut command = Command::new("notify-send");
        command.args(["--app-name", "dune-voice", "--print-id", "--transient"]);
        command.args(["--expire-time", &time.as_millis().to_string()]);
        if let Some(id) = &self.id {
            command.args(["--replace-id", id]);
        }
        command.arg(summary);
        if !body.is_empty() {
            command.arg(body);
        }
        if let Ok(out) = command.stderr(Stdio::null()).output() {
            let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !id.is_empty() {
                self.id = Some(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_lines() {
        assert_eq!(mod_change(r#"{"event":"mod","change":"start"}"#), Some(("start", "mod")));
        assert_eq!(mod_change(r#"{"event": "mod", "change": "cancel", "keys": "mod"}"#), Some(("cancel", "mod")));
        assert_eq!(mod_change(r#"{"event":"mod","change":"end","keys":"shift"}"#), Some(("end", "shift")));
        assert_eq!(mod_change(r#"{"event":"minimap","change":"toggle","output":"DP-1"}"#), None);
    }

    #[test]
    fn training_is_told_what_is_heard_then_done() {
        let (to, from) = UnixStream::pair().unwrap();
        let listening = Listening::default();
        begin_training("train\tdevtools\t2", to, false, false, &listening);
        let mut notice = Notice::new(false);
        assert!(tell_training(&listening, "", &mut notice)); // nothing heard: not counted
        assert!(tell_training(&listening, "dev tools", &mut notice));
        assert!(tell_training(&listening, "Devtools.", &mut notice));
        assert!(!tell_training(&listening, "close", &mut notice)); // done: phrases run again
        let lines: Vec<String> = BufReader::new(from).lines().map_while(Result::ok).collect();
        assert_eq!(lines, ["heard\tdev tools", "heard\tDevtools.", "done"]);
    }

    #[test]
    fn training_ends_when_train_is_gone() {
        let (to, from) = UnixStream::pair().unwrap();
        let listening = Listening::default();
        begin_training("train\tdevtools\t5", to, false, false, &listening);
        drop(from);
        let mut notice = Notice::new(false);
        assert!(!tell_training(&listening, "dev tools", &mut notice)); // it runs
        assert!(listening.training.lock().unwrap().is_none());
        assert!(!tell_training(&listening, "close", &mut notice));
    }

    #[test]
    fn hex_capture_state() {
        assert!(!capture_busy(r#"{"version":4,"capture":{"state":"idle","recording":null}}"#));
        assert!(capture_busy(r#"{"version":4,"capture":{"state":"dictating"}}"#));
        assert!(capture_busy(r#"{"capture":{"state":"transcribing"}}"#));
        assert!(!capture_busy("not json"));
    }
}
