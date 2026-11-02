//! dune-voice: say a phrase, dune does it. A daemon listens while Mod is held alone (or on
//! `dune-voice listen`), through the running Hex or Vosk, and runs the phrase's command:
//! `dune action NAME`, or `dune find` for dune-finder. Everything happens on this computer.

mod apps;
mod config;
mod context;
mod learned;
mod listen;
mod phrases;

const USAGE: &str = "\
usage:
  dune-voice           the daemon: listen each time asked (dune.toml's exec)
  dune-voice listen    listen for one phrase and run it (again while listening: stop)
  dune-voice phrases   print what can be said and what it runs
  dune-voice language CODE
                       Hex's language for dictation (Shift): en, es, it...; Mod's phrases
                       are English, so `en` again for them (each dune-voice starts in en)
  dune-voice train PHRASE... [--times N]
                       say PHRASE N times (10), holding Mod: what Hex hears instead
                       is learnt as that phrase";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => listen::serve(),
        ["listen"] => listen::ask_to_listen(),
        ["phrases"] => print_phrases(),
        ["language", code] => listen::language(code, true),
        ["train", rest @ ..] if !rest.is_empty() => train(rest),
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = result {
        eprintln!("dune-voice: {e}");
        std::process::exit(1);
    }
}

/// `dune-voice train`: hear the phrase N times, learn what Hex heard instead, tell.
fn train(args: &[&str]) -> Result<(), String> {
    let mut times = 10;
    let mut words = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if *arg == "--times" {
            times = args.next().and_then(|n| n.parse().ok()).filter(|n| (1..=50).contains(n)).ok_or("--times: 1 to 50")?;
        } else {
            words.push(*arg);
        }
    }
    let phrase = words.join(" ");
    if phrase.trim().is_empty() {
        return Err(USAGE.to_string());
    }
    println!("hold Mod and say «{phrase}», {times} times (Ctrl+C stops):");
    let heard = listen::train(&phrase, times, |i, text| println!("  {i:>2}/{times}  {text}"))?;
    if heard.len() < times {
        println!("stopped after {} of {times}", heard.len());
    }
    let config = config::load()?;
    let mut learned = learned::Learned::load();
    let kept = learned.learn(&phrase, &heard, &config.phrases);
    let right = kept.iter().filter(|(_, k)| *k == learned::Kept::Right).count();
    println!("heard right: {right} of {}", kept.len());
    let mut seen = Vec::new();
    for (text, what) in &kept {
        if seen.contains(text) {
            continue;
        }
        seen.push(text.clone());
        let n = kept.iter().filter(|(t, _)| t == text).count();
        match what {
            learned::Kept::Learned => println!("  learnt: «{text}» ({n}x)"),
            learned::Kept::Other(other) => println!("  not kept: «{text}» ({n}x) is the phrase «{other}»"),
            _ => {}
        }
    }
    learned.save()?;
    println!("{}", learned::path().display());
    Ok(())
}

/// Every phrase and its command; with Vosk, those it cannot hear marked.
fn print_phrases() -> Result<(), String> {
    let config = config::load()?;
    let vosk = config.engine == config::Engine::Vosk;
    let unknown = if vosk {
        vosk::set_log_level(vosk::LogLevel::Error);
        vosk::Model::new(config.model.to_string_lossy())
            .map(|mut model| listen::unknown_words(&mut model, &config))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let width = config.phrases.iter().map(|p| p.said.chars().count()).max().unwrap_or(0);
    for phrase in &config.phrases {
        let lost = vosk && (phrase.free() || phrase.said.split(' ').any(|word| unknown.iter().any(|u| u == word)));
        let pad = width - phrase.said.chars().count();
        println!("{}{}  {}{}", phrase.said, " ".repeat(pad), phrase.command, if lost { "   (Vosk cannot hear it)" } else { "" });
    }
    Ok(())
}
