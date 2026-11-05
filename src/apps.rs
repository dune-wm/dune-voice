//! The apps installed (their desktop entries), so a name heard as other words still opens
//! its app: Hex hears "spotifast" as "Spotify fast", and `open spotify fast` opens
//! spotifast, not Spotify. What is heard is matched letter by letter, spaces and case not
//! counting, against each app's name, program and desktop id; the closest wins when it is
//! close enough, else what was heard goes to dune-finder as it is.

use freedesktop_desktop_entry::desktop_entries;

/// How alike (0..1) what was heard and an app's name must be to be taken for it.
const ALIKE: f64 = 0.75;

/// Programs a desktop entry starts others with: their name says nothing of the app.
const LAUNCHERS: &[&str] = &["env", "sh", "bash", "flatpak", "snap", "gtk-launch", "xdg-open"];

/// An installed app: what dune-finder is given for it, and the names it goes by.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub name: String,
    pub keys: Vec<String>,
}

/// The apps of the desktop entries on disk (not the hidden ones).
pub fn installed() -> Vec<App> {
    desktop_entries(&[])
        .iter()
        .filter(|e| !e.no_display() && !e.hidden())
        .filter_map(|e| {
            let name = e.name::<&str>(&[])?.to_string();
            let program = e.exec().and_then(|x| {
                x.split_whitespace()
                    .map(|w| w.rsplit('/').next().unwrap_or(w))
                    .find(|w| !LAUNCHERS.contains(w) && !w.contains('='))
                    .map(String::from)
            });
            let id = e.id().rsplit('.').next().map(String::from);
            let keys = [Some(name.clone()), program, id].into_iter().flatten().collect();
            Some(App { name, keys })
        })
        .collect()
}

/// Letters and digits only, lower case: "Spotify fast" and "spotifyfast" are one.
fn compact(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The app `heard` names, if one is close enough: its name, for dune-finder.
pub fn closest<'a>(heard: &str, apps: &'a [App]) -> Option<&'a App> {
    let heard = compact(heard);
    if heard.is_empty() {
        return None;
    }
    let score = |app: &App| {
        app.keys
            .iter()
            .map(|k| strsim::normalized_damerau_levenshtein(&heard, &compact(k)))
            .fold(0.0, f64::max)
    };
    apps.iter()
        .map(|app| (score(app), app))
        .filter(|(s, _)| *s >= ALIKE)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, app)| app)
}

/// The app a general word means.
const MEANS: &[(&str, &str)] = &[("terminal", "wezterm")];

/// What was heard, or the app its general word means.
fn meant(heard: &str) -> &str {
    let said = compact(heard);
    MEANS.iter().find(|(word, _)| *word == said).map_or(heard, |(_, app)| app)
}

/// What to hand dune-finder for `heard`: the app it names, else `heard` as it is.
pub fn resolve(heard: &str) -> String {
    let apps = installed();
    let heard = meant(heard);
    closest(heard, &apps).map_or_else(|| heard.to_string(), |app| app.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, keys: &[&str]) -> App {
        App { name: name.into(), keys: std::iter::once(name).chain(keys.iter().copied()).map(String::from).collect() }
    }

    fn apps() -> Vec<App> {
        vec![
            app("Spotify", &["spotify", "spotify"]),
            app("spotifast", &["spotifast"]),
            app("Firefox", &["firefox"]),
            app("Vivaldi", &["vivaldi-stable", "vivaldi-stable"]),
            app("WezTerm", &["wezterm", "wezterm"]),
        ]
    }

    #[test]
    fn a_name_heard_as_two_words_is_its_app() {
        let apps = apps();
        assert_eq!(closest("Spotify fast", &apps).unwrap().name, "spotifast");
        assert_eq!(closest("spotify", &apps).unwrap().name, "Spotify");
        assert_eq!(closest("Wes term", &apps).unwrap().name, "WezTerm");
        assert_eq!(closest("vivaldi", &apps).unwrap().name, "Vivaldi");
    }

    #[test]
    fn a_terminal_is_wezterm() {
        assert_eq!(closest(meant("Terminal."), &apps()).unwrap().name, "WezTerm");
        assert_eq!(meant("firefox"), "firefox");
    }

    #[test]
    fn nothing_close_is_left_to_dune_finder() {
        let apps = apps();
        assert!(closest("calculator", &apps).is_none());
        assert!(closest("", &apps).is_none());
    }

    #[test]
    fn a_program_through_a_launcher_is_found_by_its_own_name() {
        // `env FOO=1 /usr/bin/thing %U` and `flatpak run org.x.Thing`: thing, Thing.
        let apps = installed();
        assert!(apps.iter().all(|a| a.keys.iter().all(|k| !LAUNCHERS.contains(&k.as_str()))));
    }
}

#[cfg(test)]
mod here {
    #[test]
    #[ignore = "reads this computer's apps"]
    fn what_is_heard_here() {
        let apps = super::installed();
        println!("{} apps", apps.len());
        for heard in ["Spotify fast", "spotify", "Spotify us", "firefox", "vivaldi", "wes term", "files", "terminal", "settings", "calculator"] {
            println!("{heard:14} -> {:?}", super::closest(heard, &apps).map(|a| &a.name));
        }
    }
}
