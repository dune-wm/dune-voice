# dune-voice

Drive [dune](https://github.com/dune-wm/dune) by voice. Hold Mod (dune's `modifier`, Super or Alt), say a short phrase
("desk three", "left", "close", "open firefox"), let Mod go, and dune does it. Everything
runs on this computer: Hex (the `hex` app, the default) or
[Vosk](https://alphacephei.com/vosk/) turns the sound into words, nothing goes to the network.

It is a part apart from dune: dune only has `dune action NAME` (what the key of that name in
`[keys]` does) and `dune find` (dune-finder with some text typed in). dune-voice listens and
runs commands; if it is not running, or the microphone fails, dune is not touched.

## How it works

- `dune-voice` is a daemon, waiting to be asked.
- Hold Mod alone (no other key) for 300 ms: a notification says `Listening…`; say the phrase
  and let Mod go. dune tells it through `dune events mod` (`start`, `end`); a key pressed
  meanwhile (a shortcut after all) is `cancel`, and nothing runs.
- Hold Shift alone for 300 ms: Hex dictates into the focused app while you speak; let Shift
  go and it is pasted there. A key, a button or the wheel meanwhile (a capital letter,
  Shift+wheel's sideways scroll) drops it; before the 300 ms, it never starts. A
  dictation Hex is already doing (its own shortcut, `hex dictation toggle`) is left alone.
- `dune-voice listen` (on a key of `[spawn]`, or a script) listens without holding anything,
  for `timeout` seconds (Vosk: until a short silence after a phrase); again meanwhile: it stops.
- What was heard is matched against the phrases (case, accents and punctuation do not count;
  numbers as words or digits). Only a phrase's command ever runs, through `sh -c`; the
  notification says what it heard (`✓ desk three`), `Not understood` or `Heard nothing`.
- Each dune session starts its own `dune-voice` (`exec`); one still running from before goes.

### Engines

- **hex** (default): the running Hex app hears, with its microphone and model (Parakeet,
  Whisper…): `hex dictation start --raw` when Mod goes down, `hex dictation stop --print`
  when it is let go, which prints the words and pastes nothing. It hears any words, so
  phrases can have free text (`{text}`): this is what lets you say "open firefox". Hex must
  be running (`hex start`); its own shortcut does not get in the way.
- **vosk**: Vosk inside dune-voice, on `pw-record`'s sound (16 kHz mono). It is only given the
  phrases without `{text}`, so it picks one of them or none; it needs a model in the
  language of the phrases (`model`).

## Install

```sh
git clone https://github.com/dune-wm/dune-voice && cd dune-voice
cargo install --path . --locked     # links libvosk: sudo pacman -S vosk-api
```

For Vosk only, a model (English, 40 MB; others: https://alphacephei.com/vosk/models):

```sh
mkdir -p ~/.local/share/dune-voice && cd ~/.local/share/dune-voice
curl -LO https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip
unzip vosk-model-small-en-us-0.15.zip && ln -sfn vosk-model-small-en-us-0.15 model
```

In `~/.config/dune/dune.toml`:

```toml
exec = ["dune-voice"]                      # with the rest of exec
[spawn]
"mod+period" = "dune-voice listen"         # optional: without holding Mod
```

Holding Mod or Shift needs a dune with `mod` events (Shift: with `keys`) and a `modifier` of one key (`super`, `alt`);
the finder phrases need a dune with `dune find`.

## Phrases

`dune-voice phrases` prints every phrase and its command (with Vosk, it marks those it cannot
hear). Built in:

| Phrase | Does |
|---|---|
| desk one .. nine | show that desktop |
| send to one .. nine | send the tile to that desktop |
| left / right / up / down | focus that way |
| move left / right / up / down | swap that way |
| monitor / monitor left / right | focus the next monitor / that monitor |
| send monitor | send the tile to the next monitor |
| click / right click / middle click / double click | the pointer's buttons where it is (dune-eye-tracker learns from them too) |
| zoom / zoom off | dune's last-mile zoom: the tile under the pointer magnified 3x around it, for the last pixels (then "click" clicks there and goes out) / out without a click |
| close | close the tile |
| fullscreen, full screen / full width | fullscreen / full width |
| wider / narrower / taller / shorter / cycle width | sizes |
| center / float | center / float |
| terminal / terminal below | a terminal to the right / below |
| apps / windows / options | dune-finder: apps / windows / tile options |
| finder / finder apps / finder files | dune-finder: apps / windows (its `/` mode) / files |
| next, next tab / back, previous tab / tabs / new tab | tabs (new tab: an empty one) |
| dock / map | the tile into the dock / the minimap |
| mute / screenshot / lock | mute the tile / screenshot of the monitor / lock |
| dictate english / spanish / italian | Hex's language (`dune-voice language en`, `es`, `it`) |

With dune-finder (Hex only, the text is anything said):

| Phrase | Does |
|---|---|
| open *text* | start the app it names (`open firefox`): the installed app whose name is closest to what was heard, spaces and case not counting (Hex's "Spotify fast" is spotifast), else the first dune-finder lists |
| find *text*, search *text* | dune-finder on the apps with the text typed in |
| window *text*, go to *text* | show the first window it lists (`go to slack`) |
| file *text* | dune-finder on the files with the text typed in |
| open file *text* | open the first file or folder it lists |
| new tab *text* | start the first app it lists as a tab of this tile (`new tab firefox`) |

With [dune-a11y](https://github.com/dune-wm/dune-a11y) (Hex only):

| Phrase | Does |
|---|---|
| click *text*, click on *text*, press *text* | click what is called that in the focused window: a button, a link, a tab, a menu item (`click save`, `press send`); `dune-a11y names` lists what can be said. "click" alone is still the pointer's button |

"Open" and "go to" take the first row as Enter would, never the text as a command; when
nothing matches, dune-finder stays open with the text. A phrase without free text wins over
one with it, and among those with it, the one with the most words around it ("open file
notes" is `open file {text}`).

`~/.config/dune/voice.toml` adds phrases, changes their command or removes them
([voice.example.toml](voice.example.toml)). `{n}` is a number from one to nine, `{text}`
anything said (quoted for the shell in the command).

## Languages

The phrases are English. Dictation (Shift) is in Hex's language: `dune-voice language
es` (or "dictate spanish"; `it`, "dictate italian") changes it, any language Hex's model
has (`hex model list`); a notification says it. Mod's phrases need it English again
("dictate english", `dune-voice language en`). Each `dune-voice` (each dune session)
starts in English. A change takes Hex 1.5–3 s.

## Training

When Hex hears a phrase as something else ("open the valdi" for "open vivaldi", "dev
tools" for "devtools"), teach it:

```sh
dune-voice train open vivaldi          # --times N: N times instead of 10
```

Then hold Mod and say the phrase, 10 times; nothing runs meanwhile, and a notification
counts. It prints what Hex heard each time and, at the end, keeps each that is not the
phrase as a way of saying it, in `~/.local/state/dune-voice/learned.toml` (`"heard" =
"phrase"`): heard again, it is that phrase, before anything matches (the browser's and the
minimap's phrases too). What is already another phrase ("close") is not kept, said so.
Ctrl+C stops it (what was heard so far is still learnt). Remove a line of the file to
forget it. Hex only: Vosk only hears the phrases themselves.

## What is on screen

Some phrases depend on it, and are tried before the table above:

- **The minimap open** (dune-shell's, `map` or Mod+a): `next`, `back`, `first`, `last`,
  `middle`, or a window's name (`vivaldi`, `terminal`: the first one when there are more;
  else words of its title) go to that window; `done` closes the minimap. Opened by its
  key, the minimap also takes the arrow keys (Home/End, Enter or Escape closes it).
  dune-shell writes what it shows to `$XDG_RUNTIME_DIR/dune-shell-minimap-OUTPUT.json`.
- **A Chromium browser focused** (Vivaldi, Chrome, Brave…): `open x.com` / `open github
  dot com` (that address in this tab; not an address: the app, as usual), `search …` (a
  new tab, the browser's search engine), `devtools`, `back`, `forward`, `reload`, `next` /
  `previous` (tabs), `new tab`, `close tab`, `reopen tab`, `address`, `top`, `bottom`,
  `scroll down` / `up`, `zoom in` / `out` / `reset`, `find in page …`, and `tab …` (the
  tab whose title or address has those words). Keys go through `wtype` (dune's virtual
  keyboard); `tab …` through the DevTools HTTP API: start the browser with
  `--remote-debugging-port=9222` (`DUNE_VOICE_CDP_PORT` for another port).

After the table, when no phrase is it: **the strip of tiles** of the focused monitor
(`dune events window,strip`): `first`, `last`, or a program's name (`firefox`, `wezterm`,
`gedit`: its app id has it anywhere; the leftmost one when there are more; else words of
a title) go to that tile. A phrase of the table wins: `terminal` still opens one.

## Files

- `src/main.rs`: the commands.
- `src/listen.rs`: the daemon, the socket (`$XDG_RUNTIME_DIR/dune-voice.sock`), Hex and Vosk.
- `src/phrases.rs`: the built-in phrases, `{n}`, `{text}`, matching what was heard.
- `src/config.rs`: `voice.toml`.
- `src/learned.rs`: what `train` learnt, applied to what is heard.
