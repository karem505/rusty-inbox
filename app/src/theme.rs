//! System theme. On Omarchy the current theme's `colors.toml` (rewritten by `omarchy theme set`)
//! drives every colour, and `omarchy font current` the font. Elsewhere a built-in light palette is
//! used. The palette is plain JSON so QML (`Theme.qml`) can bind to it; a watcher thread reports
//! theme switches so the UI follows them live.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Where Omarchy keeps the active theme (newest layout first).
pub fn theme_files() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("RUSTY_INBOX_THEME_FILE") {
        return vec![PathBuf::from(p)];
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/state"));
    vec![
        state.join("omarchy/current/theme/colors.toml"),
        // inside a Flatpak XDG_STATE_HOME points at the app's private dir; the host's is mounted read-only here
        home.join(".local/state/omarchy/current/theme/colors.toml"),
        home.join(".config/omarchy/current/theme/colors.toml"),
    ]
}

/// `key = "value"` lines of a flat TOML file (all Omarchy's colors.toml uses).
pub fn parse_colors(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let (k, rest) = l.split_once('=')?;
            let rest = rest.trim();
            // quoted value: up to the closing quote (a `#` inside is part of the colour);
            // bare value: up to an inline ` #` comment
            let v = match rest.strip_prefix('"') {
                Some(q) => q.split('"').next()?,
                None => rest.split(" #").next().unwrap_or(rest).trim(),
            };
            let k = k.trim();
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|x| x as f64);
    Some((c(0)?, c(2)?, c(4)?))
}

fn hex((r, g, b): (f64, f64, f64)) -> String {
    format!("#{:02x}{:02x}{:02x}", r.round() as u8, g.round() as u8, b.round() as u8)
}

/// `a` moved towards `b` by `t` (0..1).
pub fn mix(a: &str, b: &str, t: f64) -> String {
    match (rgb(a), rgb(b)) {
        (Some(x), Some(y)) => hex((x.0 + (y.0 - x.0) * t, x.1 + (y.1 - x.1) * t, x.2 + (y.2 - x.2) * t)),
        _ => a.to_string(),
    }
}

/// Relative luminance (0 dark .. 1 light).
pub fn luminance(c: &str) -> f64 {
    rgb(c).map(|(r, g, b)| (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0).unwrap_or(1.0)
}

const LIGHT: [(&str, &str); 20] = [
    ("background", "#ffffff"),
    ("dark_background", "#eceef2"),
    ("darker_background", "#2b2d33"),
    ("lighter_background", "#f4f5f8"),
    ("foreground", "#1d1d22"),
    ("dark_foreground", "#8a8d96"),
    ("light_foreground", "#5b5f6b"),
    ("bright_foreground", "#1d1d22"),
    ("accent", "#2f6bed"),
    ("selection", "#e3ebfd"),
    ("muted", "#c9ccd4"),
    ("red", "#d64545"),
    ("yellow", "#c99a16"),
    ("orange", "#e07b39"),
    ("green", "#3f9d5a"),
    ("cyan", "#2b9cb0"),
    ("blue", "#2f6bed"),
    ("magenta", "#a557c9"),
    ("mode", "light"),
    ("toast_text", "#ffffff"),
];

/// The UI palette (role -> colour) derived from a theme's colours.
pub fn palette(colors: &HashMap<String, String>, font: Option<&str>, source: &str) -> Value {
    let base: HashMap<&str, &str> = LIGHT.iter().copied().collect();
    let get = |k: &str| colors.get(k).cloned().unwrap_or_else(|| base.get(k).copied().unwrap_or("#888888").to_string());
    let bg = get("background");
    let fg = get("foreground");
    let dark = colors.get("mode").map(|m| m == "dark").unwrap_or_else(|| luminance(&bg) < 0.5);
    let accent = colors.get("accent").cloned().unwrap_or_else(|| get("blue"));
    let mut p = Map::new();
    let mut put = |k: &str, v: String| {
        p.insert(k.to_string(), Value::String(v));
    };
    put("window", bg.clone());
    put("list", bg.clone());
    put("sidebar", get("dark_background"));
    put("surface", get("lighter_background"));
    put("hover", mix(&bg, &fg, if dark { 0.07 } else { 0.035 }));
    put("selected", mix(&bg, &accent, if dark { 0.22 } else { 0.12 }));
    put("navSelected", mix(&get("dark_background"), &fg, if dark { 0.12 } else { 0.08 }));
    put("border", mix(&bg, &fg, if dark { 0.14 } else { 0.08 }));
    put("section", mix(&bg, &fg, 0.03));
    put("text", fg.clone());
    put("textStrong", get("bright_foreground"));
    put("textMuted", if dark { get("dark_foreground") } else { "#8a8d96".into() });
    put("textSubtle", get("light_foreground"));
    put("icon", get("light_foreground"));
    put("accent", accent.clone());
    put("accentText", if luminance(&accent) > 0.55 { "#1d1d22".into() } else { "#ffffff".into() });
    put("danger", get("red"));
    put("toast", if dark { get("darker_background") } else { "#2b2d33".into() });
    put("toastText", if dark { fg.clone() } else { "#ffffff".into() });
    put("toastAction", if dark { accent.clone() } else { "#8fb4ff".into() });
    let avatars: Vec<Value> =
        ["red", "orange", "yellow", "green", "cyan", "blue", "magenta"].iter().map(|k| Value::String(get(k))).collect();
    p.insert("avatars".into(), Value::Array(avatars));
    p.insert("dark".into(), Value::Bool(dark));
    p.insert("font".into(), font.map(|f| Value::String(f.to_string())).unwrap_or(Value::Null));
    p.insert("source".into(), Value::String(source.to_string()));
    Value::Object(p)
}

fn omarchy_font() -> Option<String> {
    if let Ok(f) = std::env::var("RUSTY_INBOX_FONT") {
        return Some(f).filter(|f| !f.is_empty());
    }
    let out = std::process::Command::new("omarchy").args(["font", "current"]).output().ok()?;
    let f = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !f.is_empty()).then_some(f)
}

/// Built-in dark palette for "Dark" in Settings (neutral greys, blue accent).
const DARK: [(&str, &str); 19] = [
    ("background", "#1e1f24"),
    ("dark_background", "#18191d"),
    ("darker_background", "#2c2e35"),
    ("lighter_background", "#25272d"),
    ("foreground", "#dcdde3"),
    ("dark_foreground", "#8b8e99"),
    ("light_foreground", "#a9acb7"),
    ("bright_foreground", "#f3f4f7"),
    ("accent", "#6b9bff"),
    ("selection", "#2d3a57"),
    ("muted", "#4a4d57"),
    ("red", "#ef6b6b"),
    ("yellow", "#e5c15d"),
    ("orange", "#f0965a"),
    ("green", "#6cc38a"),
    ("cyan", "#5fc4d4"),
    ("blue", "#6b9bff"),
    ("magenta", "#c48ae0"),
    ("mode", "dark"),
];

fn mode_cell() -> &'static std::sync::Mutex<String> {
    static MODE: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
    MODE.get_or_init(|| std::sync::Mutex::new("system".into()))
}

/// "system" (follow the Omarchy theme), "light" or "dark" (built-in palettes).
pub fn set_mode(mode: &str) {
    *mode_cell().lock().unwrap() = mode.to_string();
}

fn builtin(mode: &str) -> Value {
    let font = omarchy_font();
    if mode == "dark" {
        let c: HashMap<String, String> = DARK.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        palette(&c, font.as_deref(), "built-in dark")
    } else {
        palette(&HashMap::new(), font.as_deref(), "built-in light")
    }
}

// ---- the desktop's colour scheme ------------------------------------------------------------

/// Whether the desktop prefers a dark scheme: `RUSTY_INBOX_COLOR_SCHEME` (dark/light, for tests),
/// else the XDG desktop portal's `org.freedesktop.appearance color-scheme` (1 dark, 2 light, 0 none),
/// which GNOME, KDE Plasma, COSMIC and Flatpak all provide. None when there is no preference.
pub fn desktop_prefers_dark() -> Option<bool> {
    if let Ok(v) = std::env::var("RUSTY_INBOX_COLOR_SCHEME") {
        return match v.as_str() {
            "dark" => Some(true),
            "light" => Some(false),
            _ => None,
        };
    }
    portal::color_scheme().and_then(scheme_is_dark)
}

/// The portal's color-scheme value: 1 = prefer dark, 2 = prefer light, anything else = no preference.
pub fn scheme_is_dark(v: u32) -> Option<bool> {
    match v {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod portal {
    use std::time::Duration;
    use zbus::blocking::{Connection, MessageIterator};
    use zbus::zvariant::{OwnedValue, Value};

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const IFACE: &str = "org.freedesktop.portal.Settings";

    fn as_u32(v: &Value) -> Option<u32> {
        match v {
            Value::U32(n) => Some(*n),
            // the older `Read` wraps the value in one more variant
            Value::Value(inner) => as_u32(inner),
            _ => None,
        }
    }

    fn read(conn: &Connection) -> Option<u32> {
        let args = ("org.freedesktop.appearance", "color-scheme");
        let reply = conn
            .call_method(Some(DEST), PATH, Some(IFACE), "ReadOne", &args)
            .or_else(|_| conn.call_method(Some(DEST), PATH, Some(IFACE), "Read", &args))
            .ok()?;
        let v: OwnedValue = reply.body().deserialize().ok()?;
        as_u32(&v)
    }

    /// One read, bounded: a portal that has to be D-Bus-activated must not hold up start-up.
    pub fn color_scheme() -> Option<u32> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("color-scheme".into())
            .spawn(move || {
                let _ = tx.send(Connection::session().ok().and_then(|c| read(&c)));
            })
            .ok()?;
        rx.recv_timeout(Duration::from_millis(800)).ok().flatten()
    }

    /// Call `changed` with the new value each time the desktop switches scheme (the portal's
    /// `SettingChanged` signal). Returns at once; listens on its own thread.
    pub fn watch(changed: impl Fn(Option<u32>) + Send + 'static) {
        let _ = std::thread::Builder::new().name("color-scheme-watch".into()).spawn(move || {
            let Ok(conn) = Connection::session() else { return };
            let Ok(rule) = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(IFACE)
                .and_then(|b| b.member("SettingChanged"))
                .map(|b| b.build())
            else {
                return;
            };
            let Ok(signals) = MessageIterator::for_match_rule(rule, &conn, Some(16)) else { return };
            for msg in signals.flatten() {
                let Ok((ns, key, value)) = msg.body().deserialize::<(String, String, OwnedValue)>() else { continue };
                if ns == "org.freedesktop.appearance" && key == "color-scheme" {
                    changed(as_u32(&value));
                }
            }
        });
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
mod portal {
    pub fn color_scheme() -> Option<u32> {
        None
    }
    pub fn watch(_changed: impl Fn(Option<u32>) + Send + 'static) {}
}

/// The current palette: the forced mode from Settings, else the first existing Omarchy theme
/// file, else the built-in palette matching the desktop's light/dark preference.
pub fn current() -> Value {
    let mode = mode_cell().lock().unwrap().clone();
    if mode == "light" || mode == "dark" {
        return builtin(&mode);
    }
    for f in theme_files() {
        if let Ok(text) = std::fs::read_to_string(&f) {
            let colors = parse_colors(&text);
            if !colors.is_empty() {
                return palette(&colors, omarchy_font().as_deref(), &f.display().to_string());
            }
        }
    }
    if desktop_prefers_dark() == Some(true) {
        let c: HashMap<String, String> = DARK.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        return palette(&c, None, "built-in dark (desktop)");
    }
    palette(&HashMap::new(), None, "built-in")
}

fn stamp() -> Option<(PathBuf, SystemTime)> {
    theme_files().into_iter().find_map(|f| std::fs::metadata(&f).and_then(|m| m.modified()).ok().map(|t| (f, t)))
}

/// Call `changed` with the new palette whenever the theme file changes (checked every second) or the
/// desktop switches between light and dark.
pub fn watch(changed: impl Fn(Value) + Send + Sync + 'static) {
    let changed = std::sync::Arc::new(changed);
    let on_scheme = changed.clone();
    portal::watch(move |_| on_scheme(current()));
    std::thread::Builder::new()
        .name("theme-watch".into())
        .spawn(move || {
            let mut last = stamp();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let now = stamp();
                if now != last {
                    last = now;
                    // Omarchy swaps the whole directory; give it a moment to finish writing
                    std::thread::sleep(Duration::from_millis(250));
                    changed(current());
                }
            }
        })
        .expect("theme watcher thread");
}

// ---- status-bar (tray) icon ---------------------------------------------------------------

/// Monochrome tray glyph (inbox tray, the app icon's silhouette) drawn in the bar's text colour so
/// it matches every theme; `dot` adds an unread badge in that colour.
pub fn tray_svg(color: &str, dot: Option<&str>) -> String {
    let badge = dot
        .map(|d| format!(r##"<circle cx="18" cy="5" r="4" fill="{d}" stroke="{color}" stroke-width="0"/>"##))
        .unwrap_or_default();
    // the tray outline leaves a gap under the badge so the dot never overlaps a stroke
    let body = if dot.is_some() {
        r##"<path d="M12.5 4.5H6.2L3.5 12v6.5h15V11"/>"##
    } else {
        r##"<path d="M6.2 4.5h9.6l2.7 7.5v6.5h-15V12z"/>"##
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 22 22"><g fill="none" stroke="{color}" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">{body}<path d="M3.5 12.5h4.3l1.4 2.6h3.6l1.4-2.6h4.3"/></g>{badge}</svg>"##
    )
}

/// Colours of the bar the tray icon sits in: Omarchy's bar uses the theme's foreground on its
/// background, and the accent for highlights.
pub fn tray_colors(p: &Value) -> (String, String) {
    let s = |k: &str, d: &str| p[k].as_str().unwrap_or(d).to_string();
    (s("text", "#1d1d22"), s("accent", "#2f6bed"))
}

/// Write the tray icon for the current theme to the cache and return its path. The file name
/// carries a hash of the content, so a theme switch yields a new path and the bar reloads it.
pub fn tray_icon_file(unread: bool) -> Option<PathBuf> {
    let p = current();
    let (fg, accent) = tray_colors(&p);
    let svg = tray_svg(&fg, unread.then_some(accent.as_str()));
    let mut h: u64 = 1469598103934665603;
    for b in svg.bytes() {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("rusty-inbox/tray");
    std::fs::create_dir_all(&dir).ok()?;
    let f = dir.join(format!("tray-{}-{h:016x}.svg", if unread { "unread" } else { "idle" }));
    if !f.exists() {
        std::fs::write(&f, svg).ok()?;
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRUVBOX: &str = r##"mode = "dark"

accent = "#7daea3"
selection = "#504945"
background = "#282828"
dark_background = "#1e1e1e"
lighter_background = "#3c3836"
foreground = "#d4be98"
dark_foreground = "#7c6f64"
red = "#ea6962"
"##;

    #[test]
    fn parses_omarchy_colors() {
        let c = parse_colors(GRUVBOX);
        assert_eq!(c["mode"], "dark");
        assert_eq!(c["accent"], "#7daea3");
        assert_eq!(c["background"], "#282828");
        assert_eq!(c.len(), 9);
        let c = parse_colors("# comment\nkey = \"#112233\" # trailing\n\nbad line\n");
        assert_eq!(c.len(), 1);
        assert_eq!(c["key"], "#112233");
    }

    #[test]
    fn dark_theme_maps_to_roles() {
        let p = palette(&parse_colors(GRUVBOX), Some("JetBrainsMono Nerd Font"), "test");
        assert_eq!(p["dark"], true);
        assert_eq!(p["window"], "#282828");
        assert_eq!(p["sidebar"], "#1e1e1e");
        assert_eq!(p["text"], "#d4be98");
        assert_eq!(p["textMuted"], "#7c6f64");
        assert_eq!(p["accent"], "#7daea3");
        assert_eq!(p["danger"], "#ea6962");
        assert_eq!(p["font"], "JetBrainsMono Nerd Font");
        assert_eq!(p["avatars"].as_array().unwrap().len(), 7);
        // hover is between window and text, closer to the window
        assert!(luminance(p["hover"].as_str().unwrap()) > luminance("#282828"));
        assert!(luminance(p["hover"].as_str().unwrap()) < luminance("#d4be98"));
    }

    #[test]
    fn no_theme_gives_light_builtin() {
        let p = palette(&HashMap::new(), None, "built-in");
        assert_eq!(p["dark"], false);
        assert_eq!(p["window"], "#ffffff");
        assert_eq!(p["font"], Value::Null);
        assert_eq!(p["accentText"], "#ffffff");
    }

    #[test]
    fn mode_is_inferred_from_background_when_missing() {
        let mut c = parse_colors(GRUVBOX);
        c.remove("mode");
        assert_eq!(palette(&c, None, "t")["dark"], true);
        let light = parse_colors("background = \"#fbf1c7\"\nforeground = \"#3c3836\"");
        assert_eq!(palette(&light, None, "t")["dark"], false);
    }

    #[test]
    fn tray_icon_uses_theme_colours() {
        let idle = tray_svg("#d4be98", None);
        assert!(idle.contains(r##"stroke="#d4be98""##));
        assert!(!idle.contains("<circle"));
        let unread = tray_svg("#d4be98", Some("#7daea3"));
        assert!(unread.contains(r##"<circle cx="18" cy="5" r="4" fill="#7daea3""##));
        assert!(unread.starts_with("<svg") && unread.ends_with("</svg>"));
        let p = palette(&parse_colors(GRUVBOX), None, "t");
        assert_eq!(tray_colors(&p), ("#d4be98".to_string(), "#7daea3".to_string()));
    }

    #[test]
    fn builtin_dark_palette_is_dark_and_readable() {
        let c: HashMap<String, String> = DARK.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let p = palette(&c, None, "t");
        assert_eq!(p["dark"], true);
        let (bg, fg) = (p["window"].as_str().unwrap(), p["text"].as_str().unwrap());
        assert!(luminance(fg) - luminance(bg) > 0.6);
    }

    #[test]
    fn mix_and_luminance() {
        assert_eq!(mix("#000000", "#ffffff", 0.5), "#808080");
        assert_eq!(mix("#102030", "#102030", 0.3), "#102030");
        assert_eq!(mix("nonsense", "#ffffff", 0.5), "nonsense");
        assert!(luminance("#ffffff") > 0.99 && luminance("#000000") < 0.01);
    }

    /// `current()` reads process-wide env vars; tests that set them run one at a time.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn portal_color_scheme_values() {
        assert_eq!(scheme_is_dark(1), Some(true));
        assert_eq!(scheme_is_dark(2), Some(false));
        assert_eq!(scheme_is_dark(0), None);
        assert_eq!(scheme_is_dark(7), None);
    }

    #[test]
    fn system_mode_follows_the_desktop_scheme_without_omarchy() {
        let _g = env_lock();
        let missing = std::env::temp_dir().join(format!("rustyinbox-no-theme-{}/colors.toml", std::process::id()));
        std::env::set_var("RUSTY_INBOX_THEME_FILE", &missing);
        set_mode("system");
        std::env::set_var("RUSTY_INBOX_COLOR_SCHEME", "dark");
        let dark = current();
        std::env::set_var("RUSTY_INBOX_COLOR_SCHEME", "light");
        let light = current();
        std::env::set_var("RUSTY_INBOX_COLOR_SCHEME", "none");
        let none = current();
        std::env::remove_var("RUSTY_INBOX_COLOR_SCHEME");
        std::env::remove_var("RUSTY_INBOX_THEME_FILE");
        assert_eq!(dark["dark"], true);
        assert_eq!(dark["window"], DARK.iter().find(|(k, _)| *k == "background").unwrap().1);
        assert_eq!(light["dark"], false);
        assert_eq!(none["dark"], false);
    }

    #[test]
    fn an_omarchy_theme_still_wins_over_the_desktop_scheme() {
        let _g = env_lock();
        let dir = std::env::temp_dir().join(format!("rustyinbox-theme-wins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("colors.toml");
        std::fs::write(&f, GRUVBOX).unwrap();
        std::env::set_var("RUSTY_INBOX_THEME_FILE", &f);
        std::env::set_var("RUSTY_INBOX_COLOR_SCHEME", "light");
        set_mode("system");
        let p = current();
        std::env::remove_var("RUSTY_INBOX_COLOR_SCHEME");
        std::env::remove_var("RUSTY_INBOX_THEME_FILE");
        assert_eq!(p["window"], "#282828");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Against a real portal: `RUSTY_INBOX_PORTAL_PROBE=<first>,<after>` names the color-scheme values
    /// a portal on the session bus reports at start and after its first SettingChanged. Run it on a
    /// private bus with a fake portal (`dbus-run-session`); ignored in normal runs.
    #[test]
    #[ignore]
    fn live_portal_read_and_change() {
        let spec = std::env::var("RUSTY_INBOX_PORTAL_PROBE").expect("RUSTY_INBOX_PORTAL_PROBE=<first>,<after>");
        let (first, after) = spec.split_once(',').unwrap();
        let (first, after): (u32, u32) = (first.parse().unwrap(), after.parse().unwrap());
        assert_eq!(portal::color_scheme(), Some(first));
        let (tx, rx) = std::sync::mpsc::channel();
        portal::watch(move |v| {
            let _ = tx.send(v);
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(6)).unwrap(), Some(after));
        assert_eq!(portal::color_scheme(), Some(after));
    }

    #[test]
    fn current_reads_the_file_named_by_env() {
        let _g = env_lock();
        let dir = std::env::temp_dir().join(format!("rustyinbox-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("colors.toml");
        std::fs::write(&f, GRUVBOX).unwrap();
        std::env::set_var("RUSTY_INBOX_THEME_FILE", &f);
        std::env::set_var("RUSTY_INBOX_FONT", "Test Sans");
        let p = current();
        std::env::remove_var("RUSTY_INBOX_THEME_FILE");
        std::env::remove_var("RUSTY_INBOX_FONT");
        assert_eq!(p["window"], "#282828");
        assert_eq!(p["font"], "Test Sans");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
