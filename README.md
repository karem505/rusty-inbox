<div align="center">

<img src="app/packaging/icons/rusty-inbox-256.png" width="112" alt="Rusty Inbox icon">

# Rusty Inbox

<p align="center"><a href="https://github.com/sponsors/karem505"><img src="https://img.shields.io/badge/Sponsor%20this%20project-ea4aaa?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor this project on GitHub Sponsors" height="40"></a></p>

<p align="center"><b>Sponsorship goal: 10 monthly sponsors.</b> That funds one fixed day every month for Rusty Inbox releases, bug fixes and new features.<br>
<a href="https://github.com/sponsors/karem505"><img src="https://img.shields.io/github/sponsors/karem505?label=sponsors%20so%20far&style=for-the-badge&color=ea4aaa&logo=githubsponsors&logoColor=white" alt="Number of GitHub sponsors"></a></p>

**A fast, native email client for Linux, Windows and macOS.**
Rust mail engine · Qt 6 / QML interface · follows your desktop theme

[![CI](https://github.com/karem505/rusty-inbox/actions/workflows/ci.yml/badge.svg)](https://github.com/karem505/rusty-inbox/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/karem505/rusty-inbox?include_prereleases)](https://github.com/karem505/rusty-inbox/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

[Download](#install) · [Features](#features) · [Screenshots](#screenshots) · [Build from source](#build-from-source)

</div>

Rusty Inbox is an open-source desktop mail client for IMAP and SMTP accounts: Gmail (with an app
password), Fastmail, iCloud, Yahoo, cPanel and any other standard mail server. It sorts the inbox
into people, notifications and newsletters. Every action works from the keyboard. The window
starts in a fraction of a second and stays light on memory, because the whole mail engine is
native Rust and the interface is Qt Quick, not a browser shell.

▶ **Watch the 50-second launch video on YouTube** (click the image):

[![Rusty Inbox launch video on YouTube](docs/screenshots/promo-poster-play.jpg)](https://www.youtube.com/watch?v=eDgUudd3zOw)

## Features

- **Optional AI assistant (new in 0.2.0).** Choose the provider and the model: sign in with your
  ChatGPT subscription (no API key), or use OpenAI, Anthropic, OpenRouter, Ollama (local or
  cloud) or any OpenAI-compatible server. The model list is fetched live from the provider and
  is searchable, and you can set the thinking level. It proofreads with the changes highlighted,
  rewrites in six styles, translates, drafts a reply from a short instruction, summarizes a
  conversation with its action items and due dates, suggests three replies, summarizes
  attachments (PDF, DOCX, text) and checks suspicious mail for phishing. Optional background
  helpers keep a *Needs reply* list, remind you about sent mail that got no answer, answer
  plain-words questions in the search field and write a daily briefing. Everything is a preview
  until you click Replace, and it is all off until you turn it on.
- **Smart Inbox.** Mail from people comes first. Notifications, newsletters and invitations go
  into their own groups, which you can fold away.
- **Instant new mail.** Each account waits on its Inbox with IMAP IDLE, so new mail shows the
  moment the server has it. Servers without IDLE are checked every minute (or as set).
- **Unified mailboxes.** All accounts in one inbox, with per-account Sent, Drafts and Done when
  you need them.
- **Compose, reply, reply all, forward.** Rich-text editor, recipient chips with contact
  suggestions, attachments (picker or drag-and-drop), per-account signatures.
- **Undo Send and Send Later.** Messages wait in a local outbox, so you can take them back. A
  message that fails is retried by itself and kept under *Scheduled*, so nothing is lost.
- **Attachments.** Click an attachment to open it, or save it (or all of a message's attachments)
  anywhere. Files are downloaded once and kept in the local cache.
- **People.** Click any name or address in a message (From, To, Cc or Bcc) to open the person's
  card: write to them, copy the address, or list every conversation you have with them.
- **Templates** for the replies you write every week.
- **Drafts sync with the server.** Drafts save while you type and appear in your mail provider's
  Drafts folder. Sent mail is filed in Sent, and replied mail is flagged as answered.
- **Done, Snooze, Pin and bulk actions** (select all or a range, then act), each with undo.
- **Keyboard first.** `J`/`K` to move, `E` Done, `S` snooze, `R` reply, `A` reply all, `F`
  forward, `C` compose, `Ctrl+Enter` send. The full list is under *Settings → Shortcuts*.
- **Desktop native.** Status-bar (tray) icon with an unread dot, desktop notifications,
  `mailto:` links, a single running instance (launching again brings the window back), and live
  theming. On [Omarchy](https://omarchy.org) the colours and font follow `omarchy theme set` as
  you switch themes.
- **Import from Thunderbird.** Accounts, servers and saved passwords come over in one click,
  provided the profile has no primary password.
- **In-app updates.** *Settings → Updates* checks for new releases daily or weekly, shows what
  changed and installs with one click, verified against the release checksums.
- **Full HTML mail.** Each message renders in an isolated, script-free frame. Remote images
  load, and links open in your browser.
- **Local and private.** Mail is cached in SQLite on your machine. Passwords live in a local
  file readable only by you. There are no accounts, telemetry or cloud relay.

## Screenshots

| Smart Inbox | Reading |
|---|---|
| ![Smart Inbox](docs/screenshots/smart-inbox.jpg) | ![Reader](docs/screenshots/reader.jpg) |
| **Reply** | **Bulk actions** |
| ![Composer](docs/screenshots/composer.jpg) | ![Select multiple](docs/screenshots/bulk-actions.jpg) |
| **Accounts** | **Light theme** |
| ![Accounts](docs/screenshots/accounts.jpg) | ![Light theme](docs/screenshots/light-theme.jpg) |

A full tour on YouTube, recorded on a demo mailbox:

[![Rusty Inbox feature tour on YouTube](docs/screenshots/tour-poster-play.jpg)](https://www.youtube.com/watch?v=QinUtaGOhWg)

## Install

**On Linux, Flatpak is the recommended way to install.** It is signed, runs sandboxed, works on
every distribution and updates with the rest of your apps:

```sh
flatpak install --user https://karem505.github.io/rusty-inbox/rusty-inbox.flatpakref
```

Already installed? `flatpak update` brings the new version. Run it with
`flatpak run io.github.karem505.rusty-inbox` or from your app launcher.

| Platform | Package |
|---|---|
| **Linux (recommended)** | Flatpak, the command above (updates with `flatpak update`) |
| **Linux (other options)** | AppImage, Flatpak bundle or tarball from [Releases](https://github.com/karem505/rusty-inbox/releases) |
| **Arch Linux** | `makepkg -si` in [`packaging/aur/rusty-inbox`](packaging/aur/rusty-inbox) (from source) or [`rusty-inbox-bin`](packaging/aur/rusty-inbox-bin); AUR listing coming |
| **Windows 10/11** | Installer (`.exe`) or portable `.zip` from [Releases](https://github.com/karem505/rusty-inbox/releases) |
| **macOS 12+** | `.dmg` (Apple Silicon and Intel) from [Releases](https://github.com/karem505/rusty-inbox/releases) |

### One-line install and update (Linux, macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/karem505/rusty-inbox/main/scripts/install.sh | sh
```

The script picks the right package for your system: Flatpak on Linux (the AppImage when Flatpak
is not installed) and the app on macOS. An existing install keeps its method, so running it again
updates in place. Pass `--method flatpak|appimage|tarball|source` to choose one yourself.

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/karem505/rusty-inbox/main/scripts/install.ps1 | iex
```

### Updates

**Settings → Updates** shows the installed version, checks GitHub for a new release (daily or
weekly, or on demand) and shows what changed. How the update is applied depends on the install:

- **Flatpak (recommended on Linux):** the app tells you a new version is out; `flatpak update`
  (or your software center) installs it.
- **AppImage and Linux binary:** downloaded, checked against `SHA256SUMS` and swapped in place;
  restart to use it. Can also install automatically in the background.
- **Windows installer:** runs the new installer silently and reopens the app.
- **macOS:** downloads and opens the new disk image.
- **AUR / distribution packages, source builds:** the app tells you a new version is out;
  update with `flatpak update`, your package manager or `git pull`.

`rusty-inbox --check-update` does the same check from a terminal.

### Adding accounts

Open *Settings → Accounts*. Most providers are detected from the address alone. For other
servers, enter the IMAP and SMTP hosts by hand. For **Gmail**, create an
[app password](https://myaccount.google.com/apppasswords) and use it as the password. Coming
from Thunderbird? Choose *Import from Thunderbird*.

From the command line:

```sh
rusty-inbox --import-thunderbird            # every Thunderbird account not added yet
rusty-inbox --add-account=account.json      # {"email": "...", "name": "...", "passwordFile": "..."}
rusty-inbox --compose                        # new message in the running window
rusty-inbox 'mailto:someone@example.org?subject=Hi'
```

## Build from source

Requirements: Rust 1.85+, Qt 6.8+ with Qt Quick, Quick Controls 2 and **Qt WebEngine**, plus a C++
compiler. The build finds Qt through `qmake6`, `qmake`, or the `QMAKE` environment variable.

```sh
# Arch:   sudo pacman -S rust qt6-declarative qt6-webengine qt6-svg
# Fedora: sudo dnf install rust cargo qt6-qtdeclarative-devel qt6-qtwebengine-devel qt6-qtsvg-devel
# Ubuntu 24.10+: sudo apt install cargo qt6-declarative-dev qt6-webengine-dev qt6-svg-dev qml6-module-qtquick-controls qml6-module-qtwebengine qml6-module-qt-labs-platform
git clone https://github.com/karem505/rusty-inbox && cd rusty-inbox
cargo build --release
./target/release/rusty-inbox
# install for your user (binary, launcher entry, icons, mailto handler):
app/packaging/install-user.sh
```

Or let the install script do it: `scripts/install.sh --method source`.

## How it is built

```
app/                      the desktop app: Qt 6 / QML through cxx-qt
  qml/                    every screen: inbox, reader, composer, settings
  src/bridge.rs           the QObject QML talks to (JSON in, signals out)
crates/rusty-inbox-core/  the mail engine, no UI
  src/mail/               IMAP sync (async-imap + rustls), SMTP (lettre), MIME parsing
  src/store.rs            SQLite cache: threads, messages, outbox, settings
  src/compose.rs          drafts, reply / forward, mailto
  src/thunderbird.rs      Thunderbird profile import (NSS key4.db decryption)
crates/rusty-inbox-ai/    the AI assistant, no UI: providers, live model lists, streamed chat,
                          the ChatGPT sign-in (OAuth PKCE + device code), prompts and parsers
tools/demo-video/         the scripts that recorded the videos above on a fake mailbox
```

The engine runs inside the app on a small Tokio runtime. QML calls synchronous getters for
anything already stored locally, and network work reports back through Qt signals, so the
interface never waits on the network. IMAP connections are pooled per account, which keeps
servers that throttle frequent logins happy.

Tests: `cargo test --workspace`. UI smoke test, run off-screen against a copy of your data:
`QT_QPA_PLATFORM=offscreen RUSTY_INBOX_DATA_DIR=/tmp/copy target/release/rusty-inbox --autotest=/tmp/shots`.

## Privacy

Rusty Inbox talks only to the mail servers you add, plus GitHub's release API when it checks for
updates (turn that off in Settings → Updates). It has no analytics, no crash reporting and no
servers of its own. Your mail, settings and passwords stay in your data directory:
`~/.local/share/rusty-inbox` on Linux, `~/Library/Application Support/rusty-inbox` on macOS,
`%APPDATA%\rusty-inbox` on Windows.

**AI assistant.** It is off until you turn it on in Settings → AI and choose a provider. Then the
text of the mail you act on (or, for the background helpers you switch on, new mail) goes to that
provider and nowhere else: OpenAI / ChatGPT, Anthropic, OpenRouter, ollama.com, or a server you
run yourself (Ollama on this computer keeps everything local). The background helpers are off by
default and only look at mail that arrives after you switch them on. Accounts listed under
*Accounts excluded from AI* are never sent, by any feature. API keys and the ChatGPT sign-in are
kept in the same local secret store as your mail passwords, never in settings or logs.

## Contributing

Issues and pull requests are welcome. Please run `cargo fmt`, `cargo clippy --workspace -- -D warnings`
and `cargo test --workspace` before sending a change.

## License

Licensed under either the [MIT license](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE),
at your option.
