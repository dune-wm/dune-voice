//! `~/.config/dune/voice.toml` (or `$XDG_CONFIG_HOME/dune/voice.toml`), next to dune's own
//! files. Every setting is optional; it is read again each time dune-voice listens, so a
//! change applies on the next phrase (but `engine` and `model`, read when dune-voice starts).
//!
//! ```toml
//! engine = "hex"                              # who hears: "hex" (the running Hex) or "vosk"
//! model = "~/.local/share/dune-voice/model"   # vosk: a Vosk model folder
//! timeout = 5                                 # seconds `dune-voice listen` listens
//! notify = true                               # notify-send: listening, what it heard
//!
//! [phrases]                                   # on top of the built-in ones
//! "settings" = "qs -c dune ipc call settings toggle"
//! "lock" = "none"                             # a built-in one gone
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::phrases::{self, Phrase};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    engine: Option<Engine>,
    model: Option<String>,
    timeout: Option<f64>,
    notify: Option<bool>,
    phrases: BTreeMap<String, String>,
}

/// Who turns the sound into words.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// The running Hex (`hex dictation`): its model and microphone, any words (`{text}`).
    #[default]
    Hex,
    /// Vosk in dune-voice, on `pw-record`'s sound: only the phrases it is given.
    Vosk,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub engine: Engine,
    pub model: PathBuf,
    pub timeout: Duration,
    pub notify: bool,
    pub phrases: Vec<Phrase>,
}

/// Where the file is.
pub fn path() -> PathBuf {
    config_home().join("dune/voice.toml")
}

/// The settings of the file, the defaults for those it has not; no file: all defaults.
pub fn load() -> Result<Config, String> {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    from_text(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn from_text(text: &str) -> Result<Config, String> {
    let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
    let timeout = file.timeout.unwrap_or(5.0);
    if !(1.0..=30.0).contains(&timeout) {
        return Err(format!("timeout {timeout}: from 1 to 30 seconds"));
    }
    Ok(Config {
        engine: file.engine.unwrap_or_default(),
        model: file.model.map_or_else(|| data_home().join("dune-voice/model"), |m| expand(&m)),
        timeout: Duration::from_secs_f64(timeout),
        notify: file.notify.unwrap_or(true),
        phrases: phrases::table(&file.phrases),
    })
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map_or_else(|| home().join(".config"), PathBuf::from)
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map_or_else(|| home().join(".local/share"), PathBuf::from)
}

/// `~/` at the start: the home folder.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_set_is_the_defaults() {
        let config = from_text("").unwrap();
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert!(config.notify);
        assert_eq!(config.engine, Engine::Hex);
        assert!(config.model.ends_with("dune-voice/model"));
        assert!(phrases::find(&config.phrases, "close").is_some());
    }

    #[test]
    fn the_file_is_read() {
        let config = from_text(
            "engine = \"vosk\"\nmodel = \"~/m\"\ntimeout = 3\nnotify = false\n[phrases]\n\"hola\" = \"echo hola\"\n",
        )
        .unwrap();
        assert_eq!(config.engine, Engine::Vosk);
        assert_eq!(config.model, home().join("m"));
        assert_eq!(config.timeout, Duration::from_secs(3));
        assert!(!config.notify);
        assert_eq!(phrases::find(&config.phrases, "hola").unwrap().command, "echo hola");
    }

    #[test]
    fn bad_settings_are_errors() {
        assert!(from_text("timeout = 0.2").is_err());
        assert!(from_text("volume = 3").is_err());
        assert!(from_text("engine = \"whisper\"").is_err());
        assert!(from_text("[phrases]\nhola = 3").is_err());
    }
}
