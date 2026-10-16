# Demo video tooling

Records the feature tour on a fictional mailbox inside a private X display (never your desktop or
your real mail), then builds the launch video from that recording.

Requirements: `Xvfb`, `ffmpeg`, `dbus-run-session`, Python 3 with Pillow, libX11 and libXtst, and
Node for the HyperFrames render. The tour switches between Omarchy themes from `$THEMES`
(default `/usr/share/omarchy/themes`); point it at any folder of `<name>/colors.toml` files.

1. `cargo build --release -p rusty-inbox`
2. `tools/demo-video/reset.sh` seeds the demo mailbox (`seed_demo` example of rusty-inbox-core)
   and starts the private display `:78`. Files go to `$DEMO_DIR` (default `target/demo`).
3. `python3 tools/demo-video/tour.py dry` rehearses with screenshots in `$DEMO_DIR/shots`;
   `python3 tools/demo-video/tour.py record` records `raw.mkv` (60 fps, cursor drawn) and writes
   the caption timings to `marks.json`.
4. `python3 tools/demo-video/captions.py` burns the captions into `rusty-inbox-features.mp4`
   (`CAPTION_FONT` selects the font file).
5. `promo/`: cut clips from `raw.mkv` into `assets/clips/` (`ffmpeg -ss A -to B -vf setpts=PTS/2.4`),
   add `assets/fonts/`, `assets/img/icon.png` and `assets/audio/bgm.wav`, run `python3 build.py`,
   then `npx hyperframes@0.8.153 render --quality high --output renders/rusty-inbox-launch.mp4`.
