# Rusty Inbox 0.2.0: AI launch video

Same look as the 0.1.0 promo (`../promo/DESIGN.md`): warm near-black canvas `#0E0D0C`, cream ink `#F6EEE6`,
rust accent `#E8743B`, Thmanyah Sans (Black headlines, Bold kickers/chips, Regular body), IBM Plex Mono for
the install command. Real footage sits in one floating, slightly tilted window per scene; motion is expo/power
eases only (no bounce). No Rust logo, no Ferris, no other vendors' logos. Provider names appear only as plain
text chips because the product really supports them.

## Pipeline (all scripts here; big media in `~/side projects/rusty-inbox-video-media/`)
1. `prep.py` seeds a throwaway mailbox, switches the AI features on, and gives the demo app a copy of the
   owner's ChatGPT sign-in with the refresh token blanked and a fictional e-mail (so it can never rotate the
   real token). `prep.py cleanup` deletes that copy.
2. `run_all.sh` = prep + start the private display/app + wait for the background helpers + `record.py rec <beats>`.
   `record.py` drives the app with real mouse/keyboard input (libXtst, `../xd.py`), one recording per beat.
3. `gen_vo.py` (ElevenLabs "Daniel - Steady Broadcaster", `eleven_v4`, one request per sentence, Groq whisper
   round-trip + word timestamps) -> `timeline.py` (scene/sentence/word times) -> `mix.py` (VO + whoosh/tick/riser/hit,
   no music, -16 LUFS).
4. `edit.py` cuts each scene's footage from the beat recordings, retimed to the VO (answers finish streaming
   at `settle()` times; waits are sped up, never slowed). `build.py` writes the HyperFrames `index.html`.
5. `hf.sh render ...` (hyperframes@0.8.153 pinned outside git).

## Scenes
1 cover/hook · 2 choose your AI · 3 write · 4 read · 5 stay safe · 6 stay on top · 7 privacy · 8 end card.
