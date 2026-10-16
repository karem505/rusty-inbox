# Flatpak packaging

App id `io.github.karem505.RustyInbox`, runtime `org.kde.Platform//6.11` with the
`io.qt.qtwebengine.BaseApp//6.11` base app and the `rust-stable` (25.08) SDK extension.

| File | Purpose |
|---|---|
| `io.github.karem505.RustyInbox.yml` | Manifest for local and CI builds (builds the surrounding checkout) |
| `flathub/io.github.karem505.RustyInbox.yml` | Flathub variant: same build, source fetched from the git tag |
| `cargo-sources.json` | Offline crate sources generated from `Cargo.lock` |
| `*.metainfo.xml`, `*.desktop` | AppStream metadata and launcher |

## Build and install locally

```sh
flatpak install --user flathub org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install --force-clean --install-deps-from=flathub \
  build-dir packaging/flatpak/io.github.karem505.RustyInbox.yml
flatpak run io.github.karem505.RustyInbox
```

## Regenerate `cargo-sources.json` (after every `Cargo.lock` change)

```sh
python3 -m venv /tmp/fpvenv && /tmp/fpvenv/bin/pip install aiohttp tomlkit toml
curl -L -o /tmp/flatpak-cargo-generator.py \
  https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
/tmp/fpvenv/bin/python /tmp/flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
```

## Bundle

```sh
flatpak build-bundle repo rusty-inbox-0.1.0-x86_64.flatpak io.github.karem505.RustyInbox
```

The `Flatpak` workflow does this on every `v*` tag and attaches the bundle to the release.

## Permissions

- `--filesystem=~/.local/state/omarchy:ro`: read the Omarchy theme colours (`current/theme/colors.toml`).
- `--filesystem=~/.thunderbird:ro`, `~/.var/app/org.mozilla.thunderbird:ro`: one-off Thunderbird import.
- `--talk-name=org.kde.StatusNotifierWatcher`: tray icon. `--talk-name=org.freedesktop.Notifications`: new-mail notifications.

The app sets its desktop file name from the build-time variable `RUSTY_INBOX_DESKTOP_ID`
(the manifest sets it to the app id so the Wayland app id matches the `.desktop` file).
