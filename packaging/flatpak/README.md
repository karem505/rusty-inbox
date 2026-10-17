# Flatpak packaging

App id `io.github.karem505.rusty-inbox`, runtime `org.kde.Platform//6.11` with the
`io.qt.qtwebengine.BaseApp//6.11` base app and the `rust-stable` (25.08) SDK extension.

| File | Purpose |
|---|---|
| `io.github.karem505.rusty-inbox.yml` | Manifest for local and CI builds (builds the surrounding checkout) |
| `flathub/io.github.karem505.rusty-inbox.yml` | Flathub variant: same build, source fetched from the git tag |
| `cargo-sources.json` | Offline crate sources generated from `Cargo.lock` |
| `*.metainfo.xml`, `*.desktop` | AppStream metadata and launcher |

## Build and install locally

```sh
flatpak install --user flathub org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install --force-clean --install-deps-from=flathub \
  build-dir packaging/flatpak/io.github.karem505.rusty-inbox.yml
flatpak run io.github.karem505.rusty-inbox
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
flatpak build-bundle repo rusty-inbox-0.1.0-x86_64.flatpak io.github.karem505.rusty-inbox
```

The `Flatpak` workflow does this on every `v*` tag and attaches the bundle to the release.

## Permissions

- `--filesystem=~/.local/state/omarchy:ro`: read the Omarchy theme colours (`current/theme/colors.toml`).
- `--filesystem=~/.thunderbird:ro`: one-off import from a native Thunderbird profile (the Flatpak Thunderbird's data dir is not allowed by the Flathub linter, so that profile cannot be imported).
- `--talk-name=org.kde.StatusNotifierWatcher`: tray icon. `--talk-name=org.freedesktop.Notifications`: new-mail notifications.

The app sets its desktop file name from the build-time variable `RUSTY_INBOX_DESKTOP_ID`
(the manifest sets it to the app id so the Wayland app id matches the `.desktop` file).

## Self-hosted repository (GitHub Pages)

Every `v*` tag (or a manual run of the Flatpak workflow) rebuilds a signed OSTree repository and
deploys it to https://karem505.github.io/rusty-inbox/:

    flatpak install --user https://karem505.github.io/rusty-inbox/rusty-inbox.flatpakref

- Branch `stable`; the KDE runtime is pulled from Flathub (`RuntimeRepo` in the flatpakref).
- Signed with a dedicated GPG key: the public key is `repo/rusty-inbox.gpg` here; the private key
  is the `FLATPAK_GPG_PRIVATE_KEY` secret (id in `FLATPAK_GPG_KEY_ID`). Losing the key means
  users have to re-add the remote, so keep the owner's offline copy.
- The site holds only the latest build; clients still update normally (they fetch the new commit).
