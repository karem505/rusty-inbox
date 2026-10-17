//! Update checks and updates from GitHub Releases.
//!
//! How an update is applied depends on how the app was installed (see [`Method`]):
//! an AppImage or a plain binary (the tarball / install script) is replaced in place and the new
//! version starts on the next launch; Windows runs the new installer; macOS downloads and opens
//! the disk image. Flatpak, distribution packages and source builds are updated by their own
//! tools, so for those the app only reports that a new version exists.
//!
//! Every download is checked against the release's `SHA256SUMS` before anything is replaced.
//! `RUSTY_INBOX_UPDATE_FEED` points the checker at another release JSON (a URL or a local file,
//! whose asset URLs may also be local paths); the tests and the scripted UI check use it.

use semver::Version;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const REPO: &str = "karem505/rusty-inbox";
const LAST_CHECK_KEY: &str = "rustyInbox.lastUpdateCheck";

// ---- how the app was installed ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Method {
    Flatpak,
    /// the AppImage file to replace (`$APPIMAGE`)
    AppImage(PathBuf),
    /// a plain executable in a folder we can write (tarball, install script): replaced in place
    Binary(PathBuf),
    WindowsInstaller,
    WindowsPortable,
    MacDmg,
    /// installed by the system package manager (pacman, apt, ...)
    Package,
    /// running from a source checkout (`target/release`)
    Source,
}

impl Method {
    pub fn id(&self) -> &'static str {
        match self {
            Method::Flatpak => "flatpak",
            Method::AppImage(_) => "appimage",
            Method::Binary(_) => "binary",
            Method::WindowsInstaller => "windows",
            Method::WindowsPortable => "windows-portable",
            Method::MacDmg => "macos",
            Method::Package => "package",
            Method::Source => "source",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Method::Flatpak => "Flatpak",
            Method::AppImage(_) => "AppImage",
            Method::Binary(_) => "Linux binary",
            Method::WindowsInstaller => "Windows installer",
            Method::WindowsPortable => "Windows portable",
            Method::MacDmg => "macOS app",
            Method::Package => "System package",
            Method::Source => "Built from source",
        }
    }

    /// The file is swapped while the app runs; the new version starts on the next launch.
    pub fn in_place(&self) -> bool {
        matches!(self, Method::AppImage(_) | Method::Binary(_))
    }

    /// The app can download and apply the update itself.
    pub fn can_install(&self) -> bool {
        match self {
            Method::AppImage(_) | Method::Binary(_) => std::env::consts::ARCH == "x86_64",
            Method::WindowsInstaller | Method::MacDmg => true,
            _ => false,
        }
    }

    /// What to do when the app cannot update itself.
    pub fn how_to(&self) -> &'static str {
        match self {
            Method::Flatpak => "Installed with Flatpak: updates arrive with `flatpak update` or your software center.",
            Method::Package => "Installed by your package manager: update it there (for example `yay -Syu`).",
            Method::Source => "Built from source: pull the latest code and rebuild, or install a release build.",
            Method::WindowsPortable => {
                "Portable copy: download the new zip from the release page and replace this folder."
            }
            _ if !self.can_install() => "There is no ready-made build for this processor; build from source.",
            _ => "",
        }
    }

    /// Release asset this install method needs for `v`.
    pub fn asset_name(&self, v: &Version) -> Option<String> {
        match self {
            Method::AppImage(_) => Some(format!("rusty-inbox-{v}-x86_64.AppImage")),
            Method::Binary(_) => Some(format!("rusty-inbox-{v}-linux-x86_64.tar.gz")),
            Method::WindowsInstaller => Some(format!("rusty-inbox-{v}-windows-x64-setup.exe")),
            Method::MacDmg => {
                let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x86_64" };
                Some(format!("rusty-inbox-{v}-macos-{arch}.dmg"))
            }
            _ => None,
        }
    }
}

/// How this copy was installed.
pub fn detect() -> Method {
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Method::Flatpak;
    }
    if let Some(p) = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty()) {
        return Method::AppImage(PathBuf::from(p));
    }
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).unwrap_or_default();
    detect_for(&exe)
}

fn detect_for(exe: &Path) -> Method {
    if cfg!(windows) {
        return if exe.parent().is_some_and(|d| d.join("unins000.exe").exists()) {
            Method::WindowsInstaller
        } else {
            Method::WindowsPortable
        };
    }
    if cfg!(target_os = "macos") {
        return Method::MacDmg;
    }
    let s = exe.to_string_lossy();
    if s.contains("/target/release/") || s.contains("/target/debug/") {
        Method::Source
    } else if ["/usr/", "/opt/", "/nix/", "/snap/"].iter().any(|p| s.starts_with(p))
        || !exe.parent().is_some_and(dir_writable)
    {
        Method::Package
    } else {
        Method::Binary(exe.to_path_buf())
    }
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".rusty-inbox-write-test-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

// ---- releases --------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    pub name: String,
    pub notes: String,
    pub url: String,
    pub published: String,
    pub assets: Vec<Asset>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// A GitHub release (REST API shape).
pub fn parse_release(v: &Value) -> Option<Release> {
    if v["draft"] == json!(true) || v["prerelease"] == json!(true) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    let version = Version::parse(tag.trim_start_matches('v')).ok()?;
    let assets = v["assets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x["name"].as_str()?.to_string(),
                        url: x["browser_download_url"].as_str()?.to_string(),
                        size: x["size"].as_u64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        version,
        name: v["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(tag).to_string(),
        notes: v["body"].as_str().unwrap_or_default().to_string(),
        url: v["html_url"].as_str().unwrap_or_default().to_string(),
        published: v["published_at"].as_str().unwrap_or_default().to_string(),
        assets,
    })
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}

fn feed() -> String {
    std::env::var("RUSTY_INBOX_UPDATE_FEED")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("https://api.github.com/repos/{REPO}/releases/latest"))
}

/// A path or `file://` URL (used by tests and the scripted UI check), else `None`.
fn local_path(url: &str) -> Option<PathBuf> {
    if let Some(p) = url.strip_prefix("file://") {
        Some(PathBuf::from(p))
    } else if !url.contains("://") {
        Some(PathBuf::from(url))
    } else {
        None
    }
}

fn agent(body_limit: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("{}/{}", crate::APP_ID, env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(body_limit))
        .build()
        .into()
}

fn fetch_text(url: &str) -> Result<String, String> {
    if let Some(p) = local_path(url) {
        return std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()));
    }
    if std::env::var_os("RUSTY_INBOX_OFFLINE").is_some() {
        return Err("offline".into());
    }
    let mut resp = agent(Duration::from_secs(60))
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// The newest published release.
pub fn fetch_latest() -> Result<Release, String> {
    let text = fetch_text(&feed())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    parse_release(&v).ok_or_else(|| "no published release found".to_string())
}

/// Download `url` to `dest`, reporting `(received, total)`.
fn download(url: &str, dest: &Path, total_hint: u64, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let mut out = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let (mut reader, total): (Box<dyn Read>, u64) = match local_path(url) {
        Some(p) => {
            let f = std::fs::File::open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let len = f.metadata().map(|m| m.len()).unwrap_or(total_hint);
            (Box::new(f), len)
        }
        None => {
            let resp = agent(Duration::from_secs(60 * 60)).get(url).call().map_err(|e| e.to_string())?;
            let len = resp
                .headers()
                .get("content-length")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(total_hint);
            (Box::new(resp.into_body().into_with_config().limit(u64::MAX).reader()), len)
        }
    };
    let mut buf = vec![0u8; 256 * 1024];
    let mut got = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", dest.display()))?;
        got += n as u64;
        progress(got, total);
    }
    out.sync_all().map_err(|e| e.to_string())?;
    if total > 0 && got != total {
        return Err(format!("download incomplete ({got} of {total} bytes)"));
    }
    Ok(())
}

/// The expected SHA-256 of `name` in a `sha256sum` listing.
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

fn sha256_file(p: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn verify(release: &Release, name: &str, file: &Path) -> Result<(), String> {
    let Some(sums) = release.asset("SHA256SUMS") else {
        tracing::warn!("release {} has no SHA256SUMS; {name} not verified", release.version);
        return Ok(());
    };
    let text = fetch_text(&sums.url)?;
    let want = checksum_for(&text, name).ok_or_else(|| format!("SHA256SUMS does not list {name}"))?;
    let got = sha256_file(file)?;
    if got != want {
        return Err(format!("{name} is damaged (checksum mismatch); nothing was changed"));
    }
    Ok(())
}

// ---- installing ------------------------------------------------------------------------------

/// What happens after a successful install.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// replaced in place: launch this file to run the new version
    Restart(PathBuf),
    /// the installer was started and replaces the app: quit now
    Quit,
    /// the user finishes it (e.g. drags the app out of the opened disk image)
    Opened(PathBuf),
}

pub fn install(method: &Method, release: &Release, progress: &mut dyn FnMut(u64, u64)) -> Result<Outcome, String> {
    if !method.can_install() {
        return Err(method.how_to().to_string());
    }
    let name = method.asset_name(&release.version).ok_or("no download for this install type")?;
    let asset = release.asset(&name).ok_or_else(|| format!("this release has no {name}"))?;
    match method {
        Method::AppImage(target) => {
            let dir = target.parent().ok_or("bad AppImage path")?;
            let part = dir.join(".rusty-inbox-update.part");
            let r = download(&asset.url, &part, asset.size, progress)
                .and_then(|_| verify(release, &name, &part))
                .and_then(|_| replace_executable(&part, target));
            let _ = std::fs::remove_file(&part);
            r?;
            write_version_file(dir, &release.version);
            Ok(Outcome::Restart(target.clone()))
        }
        Method::Binary(exe) => {
            let dir = exe.parent().ok_or("bad executable path")?;
            let work = dir.join(".rusty-inbox-update");
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
            let r = (|| {
                let tgz = work.join("update.tar.gz");
                download(&asset.url, &tgz, asset.size, progress)?;
                verify(release, &name, &tgz)?;
                let st = std::process::Command::new("tar")
                    .arg("-xzf")
                    .arg(&tgz)
                    .arg("-C")
                    .arg(&work)
                    .stdin(std::process::Stdio::null())
                    .status()
                    .map_err(|e| format!("tar: {e}"))?;
                if !st.success() {
                    return Err("could not unpack the update".to_string());
                }
                let new = find_file(&work, "rusty-inbox", 3).ok_or("the update has no rusty-inbox program")?;
                replace_executable(&new, exe)
            })();
            let _ = std::fs::remove_dir_all(&work);
            r?;
            write_version_file(dir, &release.version);
            Ok(Outcome::Restart(exe.clone()))
        }
        Method::WindowsInstaller => {
            let setup = std::env::temp_dir().join(&name);
            download(&asset.url, &setup, asset.size, progress)?;
            verify(release, &name, &setup)?;
            std::process::Command::new(&setup)
                .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
                .spawn()
                .map_err(|e| format!("could not start the installer: {e}"))?;
            Ok(Outcome::Quit)
        }
        Method::MacDmg => {
            let dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
            let dmg = dir.join(&name);
            download(&asset.url, &dmg, asset.size, progress)?;
            verify(release, &name, &dmg)?;
            let _ = std::process::Command::new("open").arg(&dmg).spawn();
            Ok(Outcome::Opened(dmg))
        }
        _ => Err(method.how_to().to_string()),
    }
}

/// Move `new` over `target` atomically (a running program can be replaced this way on Unix).
fn replace_executable(new: &Path, target: &Path) -> Result<(), String> {
    let dir = target.parent().ok_or("bad path")?;
    let staged = dir.join(".rusty-inbox.new");
    std::fs::copy(new, &staged).map_err(|e| format!("{}: {e}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&staged, target).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("could not replace {}: {e}", target.display())
    })
}

/// The install script keeps the installed version next to the program.
fn write_version_file(dir: &Path, v: &Version) {
    let f = dir.join("version");
    if f.exists() {
        let _ = std::fs::write(f, format!("{v}\n"));
    }
}

fn find_file(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_file() && p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
        if p.is_dir() && depth > 0 {
            if let Some(f) = find_file(&p, name, depth - 1) {
                return Some(f);
            }
        }
    }
    None
}

/// Start `exe` once this process has gone (the single-instance socket must be free first).
pub fn relaunch(exe: &Path) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 1; exec \"$0\"")
            .arg(exe)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new(exe).spawn().is_ok()
    }
}

// ---- shared state, shown on the Updates page -------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Status {
    /// idle | checking | current | available | downloading | ready | error
    state: &'static str,
    latest: Option<Release>,
    received: u64,
    total: u64,
    error: String,
    restart: Option<PathBuf>,
    quit: bool,
    opened: String,
}

fn status() -> &'static Mutex<Status> {
    static S: OnceLock<Mutex<Status>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Status { state: "idle", ..Default::default() }))
}

static BUSY: AtomicBool = AtomicBool::new(false);

fn method() -> &'static Method {
    static M: OnceLock<Method> = OnceLock::new();
    M.get_or_init(detect)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn last_check_ms() -> i64 {
    crate::engine::core().store().setting(LAST_CHECK_KEY).ok().flatten().and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Everything the Updates page shows.
pub fn status_json() -> Value {
    let s = status().lock().map(|s| s.clone()).unwrap_or_default();
    let m = method();
    json!({
        "current": env!("CARGO_PKG_VERSION"),
        "state": s.state,
        "latest": s.latest.as_ref().map(|r| json!({
            "version": r.version.to_string(),
            "name": r.name,
            "notes": r.notes,
            "url": r.url,
            "published": r.published,
        })),
        "received": s.received,
        "total": s.total,
        "error": s.error,
        "restart": s.restart.is_some(),
        "quit": s.quit,
        "opened": s.opened,
        "lastCheck": last_check_ms(),
        "method": { "id": m.id(), "label": m.label(), "canInstall": m.can_install(), "inPlace": m.in_place(), "howTo": m.how_to() },
        "releasesUrl": format!("https://github.com/{REPO}/releases"),
    })
}

fn set(f: impl FnOnce(&mut Status)) {
    if let Ok(mut s) = status().lock() {
        f(&mut s);
    }
}

/// Ask GitHub for the newest release. Returns true when a newer version is available.
pub fn check(report: &dyn Fn()) -> bool {
    if BUSY.swap(true, Ordering::SeqCst) {
        return false;
    }
    let ready = status().lock().map(|s| s.state == "ready").unwrap_or(false);
    set(|s| {
        if !ready {
            s.state = "checking";
            s.error.clear();
        }
    });
    report();
    let r = fetch_latest();
    let _ = crate::engine::core().store().set_setting(LAST_CHECK_KEY, &json!(now_ms()));
    let newer = match r {
        Ok(rel) => {
            let newer = rel.version > current_version();
            set(|s| {
                if !ready {
                    s.state = if newer { "available" } else { "current" };
                }
                s.latest = Some(rel);
            });
            newer && !ready
        }
        Err(e) => {
            tracing::warn!("update check: {e}");
            set(|s| {
                if !ready {
                    s.state = "error";
                    s.error = e;
                }
            });
            false
        }
    };
    BUSY.store(false, Ordering::SeqCst);
    report();
    newer
}

/// Download and apply the release found by the last check.
pub fn install_latest(report: &dyn Fn()) {
    let Some(rel) = status().lock().ok().and_then(|s| s.latest.clone()) else { return };
    if rel.version <= current_version() || BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    set(|s| {
        s.state = "downloading";
        s.received = 0;
        s.total = 0;
        s.error.clear();
    });
    report();
    let mut last = std::time::Instant::now();
    let mut progress = |got: u64, total: u64| {
        set(|s| {
            s.received = got;
            s.total = total;
        });
        if last.elapsed() > Duration::from_millis(150) {
            last = std::time::Instant::now();
            report();
        }
    };
    let r = install(method(), &rel, &mut progress);
    set(|s| match r {
        Ok(Outcome::Restart(p)) => {
            tracing::info!("installed {} to {}", rel.version, p.display());
            s.state = "ready";
            s.restart = Some(p);
        }
        Ok(Outcome::Quit) => {
            s.state = "ready";
            s.quit = true;
        }
        Ok(Outcome::Opened(p)) => {
            s.state = "ready";
            s.opened = p.display().to_string();
        }
        Err(e) => {
            tracing::warn!("update install: {e}");
            s.state = "error";
            s.error = e;
        }
    });
    BUSY.store(false, Ordering::SeqCst);
    report();
}

/// Relaunch into the installed update. False when there is nothing to restart into.
pub fn restart() -> bool {
    let p = status().lock().ok().and_then(|s| s.restart.clone());
    p.is_some_and(|p| relaunch(&p))
}

/// Background checks: shortly after start, then hourly whether the chosen interval has passed.
/// `report(newly_available)` runs after every change; with automatic installs on, an in-place
/// update is downloaded and applied right away (it takes effect on the next launch).
pub fn start_auto(report: impl Fn(bool) + Send + Sync + 'static) {
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(45));
            loop {
                let store = crate::engine::core().store();
                let prefs = crate::settings::all(store);
                let days = prefs["updateCheckDays"].as_i64().unwrap_or(1).max(1);
                let due = now_ms() - last_check_ms() >= days * 86_400_000;
                if prefs["autoUpdateCheck"] == json!(true) && due {
                    let newer = check(&|| report(false));
                    if newer {
                        report(true);
                        if prefs["autoInstallUpdates"] == json!(true) && method().in_place() && method().can_install() {
                            install_latest(&|| report(false));
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(60 * 60));
            }
        })
        .expect("update thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str, assets: &[(&str, &str)]) -> Value {
        json!({
            "tag_name": tag, "name": format!("Rusty Inbox {tag}"), "body": "**Fixes**\n- one",
            "html_url": "https://example.com/r", "published_at": "2026-10-17T10:00:00Z",
            "assets": assets.iter().map(|(n, u)| json!({"name": n, "browser_download_url": u, "size": 0})).collect::<Vec<_>>()
        })
    }

    #[test]
    fn releases_parse_and_compare() {
        let r = parse_release(&release_json("v9.1.0", &[("SHA256SUMS", "x")])).unwrap();
        assert_eq!(r.version, Version::new(9, 1, 0));
        assert!(r.version > current_version());
        assert_eq!(r.assets.len(), 1);
        let mut pre = release_json("v9.2.0", &[]);
        pre["prerelease"] = json!(true);
        assert!(parse_release(&pre).is_none());
        assert!(parse_release(&json!({"tag_name": "nightly"})).is_none());
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let sums = "ABC123  rusty-inbox-1.0.0-x86_64.AppImage\ndef456 *rusty-inbox-1.0.0-linux-x86_64.tar.gz\n";
        assert_eq!(checksum_for(sums, "rusty-inbox-1.0.0-x86_64.AppImage").as_deref(), Some("abc123"));
        assert_eq!(checksum_for(sums, "rusty-inbox-1.0.0-linux-x86_64.tar.gz").as_deref(), Some("def456"));
        assert_eq!(checksum_for(sums, "rusty-inbox-1.0.0.dmg"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_types_are_recognised() {
        assert_eq!(detect_for(Path::new("/usr/bin/rusty-inbox")), Method::Package);
        assert_eq!(detect_for(Path::new("/home/u/src/rusty-inbox/target/release/rusty-inbox")), Method::Source);
        let d = tempdir("detect");
        let exe = d.join("rusty-inbox");
        assert_eq!(detect_for(&exe), Method::Binary(exe.clone()));
        let v = Version::new(1, 2, 3);
        assert_eq!(Method::Binary(exe).asset_name(&v).unwrap(), "rusty-inbox-1.2.3-linux-x86_64.tar.gz");
        assert_eq!(Method::AppImage("/a".into()).asset_name(&v).unwrap(), "rusty-inbox-1.2.3-x86_64.AppImage");
        assert!(Method::Flatpak.asset_name(&v).is_none() && !Method::Flatpak.can_install());
    }

    #[cfg(unix)]
    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ri-upd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_is_replaced_only_when_the_checksum_matches() {
        let d = tempdir("appimage");
        let target = d.join("rusty-inbox.AppImage");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(d.join("version"), "0.1.0\n").unwrap();
        let new = d.join("dl-new.AppImage");
        std::fs::write(&new, b"new build").unwrap();
        let name = "rusty-inbox-9.0.0-x86_64.AppImage";
        let sums = d.join("SHA256SUMS");
        let new_url = new.display().to_string();
        let sums_url = sums.display().to_string();
        let rel = parse_release(&release_json("v9.0.0", &[(name, &new_url), ("SHA256SUMS", &sums_url)])).unwrap();
        let m = Method::AppImage(target.clone());

        std::fs::write(&sums, format!("{}  {name}\n", sha(b"something else"))).unwrap();
        let err = install(&m, &rel, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old", "a bad download must not replace anything");

        std::fs::write(&sums, format!("{}  {name}\n", sha(b"new build"))).unwrap();
        let mut seen = 0;
        let out = install(&m, &rel, &mut |got, _| seen = got).unwrap();
        assert_eq!(out, Outcome::Restart(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), b"new build");
        assert_eq!(seen, 9);
        assert_eq!(std::fs::read_to_string(d.join("version")).unwrap(), "9.0.0\n");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o111, 0o111);
        assert!(!d.join(".rusty-inbox-update.part").exists() && !d.join(".rusty-inbox.new").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_binary_install_is_updated_from_the_tarball() {
        let d = tempdir("tarball");
        let bin = d.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("rusty-inbox-bin");
        std::fs::write(&exe, b"old").unwrap();
        let pkg = d.join("src/rusty-inbox-9.0.0-linux-x86_64");
        std::fs::create_dir_all(pkg.join("icons")).unwrap();
        std::fs::write(pkg.join("rusty-inbox"), b"new binary").unwrap();
        let name = "rusty-inbox-9.0.0-linux-x86_64.tar.gz";
        let tgz = d.join(name);
        let st = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(d.join("src"))
            .arg("rusty-inbox-9.0.0-linux-x86_64")
            .status()
            .unwrap();
        assert!(st.success());
        let sums = d.join("SHA256SUMS");
        std::fs::write(&sums, format!("{}  {name}\n", sha(&std::fs::read(&tgz).unwrap()))).unwrap();
        let (u1, u2) = (tgz.display().to_string(), sums.display().to_string());
        let rel = parse_release(&release_json("v9.0.0", &[(name, &u1), ("SHA256SUMS", &u2)])).unwrap();
        let out = install(&Method::Binary(exe.clone()), &rel, &mut |_, _| {}).unwrap();
        assert_eq!(out, Outcome::Restart(exe.clone()));
        assert_eq!(std::fs::read(&exe).unwrap(), b"new binary");
        assert!(!bin.join(".rusty-inbox-update").exists());
    }

    /// Real download from GitHub (network): `RI_E2E_FEED=<release json> cargo test -- --ignored e2e`
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn e2e_download_and_replace_from_a_feed() {
        let Ok(feed) = std::env::var("RI_E2E_FEED") else { return };
        std::env::set_var("RUSTY_INBOX_UPDATE_FEED", feed);
        let rel = fetch_latest().unwrap();
        let d = tempdir("e2e");
        let target = d.join("rusty-inbox.AppImage");
        std::fs::write(&target, b"old").unwrap();
        let t = std::time::Instant::now();
        let mut last = 0;
        let out = install(&Method::AppImage(target.clone()), &rel, &mut |got, _| last = got).unwrap();
        assert_eq!(out, Outcome::Restart(target.clone()));
        let len = std::fs::metadata(&target).unwrap().len();
        assert_eq!(len, last);
        eprintln!("downloaded and verified {len} bytes in {:?}", t.elapsed());
        let _ = std::fs::remove_dir_all(&d);
    }
}
