# Flathub submission checklist (not yet submitted)

Docs checked: <https://docs.flathub.org/docs/for-app-authors/submission> and `/requirements`.

## Read first: Flathub's generative-AI policy

- Flathub manifests **must not contain AI-generated or AI-assisted content**, and AI tools/agents **must not open
  the submission PR or write its description, commit messages or replies**.
- The files in this directory were drafted with an AI agent. Before submitting, a human must **write the Flathub
  manifest (`flathub/*.yml`) and the PR text by hand**, treat the files here only as a reference, and **disclose
  AI-generated parts** of the application, metainfo and desktop file (which parts, approximate extent) in the PR.
- Undisclosed AI material can get the submission rejected.

## Prerequisites in this repo

1. Merge the `flatpak` PR into `main`; the metainfo, desktop file and icons must exist at the tag.
2. Tag the merge commit `v0.1.0`, push the tag, and get its full SHA: `git rev-parse v0.1.0^{commit}`.
   (The tag also triggers the `Flatpak` workflow that attaches the `.flatpak` bundle to the release.)
3. App id vs. repository: for `io.github.<user>.<Name>` Flathub expects `https://github.com/karem505/RustyInbox`.
   The repo is `karem505/rusty-inbox`, so either rename the repo to `RustyInbox` (GitHub keeps redirects) or
   expect to ask the reviewers for an exception. The metainfo URLs would then also change.
4. Lint the final files again (see below). A Flathub-quality screenshot set is already served from `main`.

## Steps

```sh
gh repo fork --clone flathub/flathub && cd flathub && git checkout --track origin/new-pr
git checkout -b add-io.github.karem505.RustyInbox new-pr
```

Add the files at the root of the branch (no subfolder):

```sh
cp <rusty-inbox>/packaging/flatpak/flathub/io.github.karem505.RustyInbox.yml .
cp <rusty-inbox>/packaging/flatpak/cargo-sources.json .
# edit the manifest: commit: <full SHA of v0.1.0>
git add io.github.karem505.RustyInbox.yml cargo-sources.json
git commit -m "Add io.github.karem505.RustyInbox" && git push -u origin HEAD
```

Open the PR in the **web UI** against base branch **`new-pr`** (not `master`), title:
`Add io.github.karem505.RustyInbox`, fill in Flathub's PR template by hand. After review, comment `bot, build`
to start a test build. On acceptance you get an invite to `flathub/io.github.karem505.RustyInbox` (enable 2FA;
accept within a week), then verify the app id (Developer Portal; `io.github.karem505` is verified through the
GitHub login method).

## Local checks before opening the PR

```sh
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.karem505.RustyInbox.yml
flatpak run --command=flathub-build org.flatpak.Builder --install io.github.karem505.RustyInbox.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

## Notes for the PR text (justification of permissions; rewrite in your own words)

- `--share=network`: IMAP/SMTP connections to the user's mail servers, remote images in mail (WebEngine).
- `--share=ipc`, `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`: GUI and GPU-accelerated Qt Quick / QtWebEngine.
- `--talk-name=org.freedesktop.Notifications`: new-mail notifications (notify-rust).
- `--talk-name=org.kde.StatusNotifierWatcher`: tray icon (Qt.labs.platform SystemTrayIcon uses StatusNotifierItem).
- `--filesystem=xdg-state/omarchy:ro`: the app follows the Omarchy desktop theme by reading
  `~/.local/state/omarchy/current/theme/colors.toml`. No XDG portal exposes this file.
  Read-only, one directory. Without it the app falls back to a built-in palette.
- `--filesystem=~/.thunderbird:ro` and `~/.var/app/org.mozilla.thunderbird:ro`: the "Import from Thunderbird" feature
  reads profile data (accounts, and saved passwords the user chooses to import) from the Thunderbird profile
  (native and Flatpak locations). Read-only; used only on explicit user action. A portal folder chooser cannot be
  used because the location is fixed and the importer scans for the profile. Be ready to drop these if reviewers
  object, and let the user pick the profile folder with the file chooser portal instead.
- File attachments use the QtQuick.Dialogs FileDialog, which goes through the xdg-desktop-portal inside Flatpak.
- No `--filesystem=home`, no `--socket=session-bus`.
