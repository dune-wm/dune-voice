//! dune-voice: say a phrase, dune does it. A daemon listens while Mod is held alone (or on
//! `dune-voice listen`), through the running Hex or Vosk, and runs the phrase's command:
//! `dune action NAME`, or `dune find` for dune-finder. Everything happens on this computer.

mod apps;
mod config;
mod context;
mod learned;
mod listen;
mod misheard;
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
                       is learnt as that phrase
  dune-voice fix       go through what was not understood: pick the phrase it was (or
                       write it), learnt as that phrase for the next time";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => listen::serve(),
        ["listen"] => listen::ask_to_listen(),
        ["phrases"] => print_phrases(),
        ["language", code] => listen::language(code, true),
        ["train", rest @ ..] if !rest.is_empty() => train(rest),
        ["fix"] => fix(),
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

/// `dune-voice fix`: each thing not understood (the most repeated first), the closest
/// phrases to pick, or one to write; learnt (or forgotten) and gone from the log.
fn fix() -> Result<(), String> {
    use std::io::{BufRead, Write};

    let path = misheard::path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let pending = misheard::pending(&text);
    if pending.is_empty() {
        println!("nothing not understood ({})", path.display());
        return Ok(());
    }
    let config = config::load()?;
    let candidates = misheard::candidates(&config.phrases);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut learned = learned::Learned::load();
    let mut text = text;
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut ask = |prompt: &str| -> Option<String> {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        lines.next().and_then(Result::ok).map(|l| l.trim().to_string())
    };
    println!("{} not understood; for each: a number, w write it, s skip, f forget, q quit", pending.len());
    for item in &pending {
        let times = if item.count == 1 { "once".to_string() } else { format!("{} times", item.count) };
        println!("\n«{}»  ({times}, {})", item.heard, misheard::ago(item.last, now));
        let offered = misheard::suggest(&item.key, &candidates, 5);
        for (i, phrase) in offered.iter().enumerate() {
            println!("  {}) {phrase}", i + 1);
        }
        let phrase = loop {
            let Some(answer) = ask("> ") else { return Ok(()) };
            match answer.as_str() {
                "q" => return Ok(()),
                "s" | "" => break None,
                "f" => {
                    text = misheard::without(&text, &item.key);
                    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
                    println!("  forgotten");
                    break None;
                }
                "w" => {
                    let Some(written) = ask("  the phrase: ") else { return Ok(()) };
                    if written.is_empty() {
                        continue;
                    }
                    if !misheard::sayable(&config.phrases, &written) {
                        let sure = ask(&format!("  no phrase is «{written}» (`dune-voice phrases`): keep it anyway? [y/N] "));
                        if sure.as_deref() != Some("y") {
                            continue;
                        }
                    }
                    break Some(written);
                }
                n => match n.parse::<usize>().ok().and_then(|n| offered.get(n.wrapping_sub(1))) {
                    Some(phrase) => break Some(phrase.clone()),
                    None => println!("  a number from 1 to {}, w, s, f or q", offered.len()),
                },
            }
        };
        let Some(phrase) = phrase else { continue };
        match learned.teach(&item.heard, &phrase, &config.phrases) {
            learned::Kept::Learned => {
                learned.save()?;
                text = misheard::without(&text, &item.key);
                std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("  ✓ «{}» = «{phrase}»", item.key);
            }
            learned::Kept::Other(other) => println!("  not kept: «{}» is the phrase «{other}» already", item.key),
            learned::Kept::Right | learned::Kept::Nothing => println!("  nothing to learn"),
        }
    }
    println!("\n{}", learned::path().display());
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
