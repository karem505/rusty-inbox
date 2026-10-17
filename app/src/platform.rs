//! Small desktop integrations that Qt does not cover.

/// Bring this process's window to the front where the compositor would otherwise refuse:
/// Hyprland ignores activation requests from background apps (e.g. a window shown from the tray
/// by a keyboard shortcut), so ask it directly through `hyprctl`. Elsewhere this does nothing.
pub fn focus_own_window() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return;
    }
    let pid = std::process::id();
    std::thread::spawn(move || {
        // give the compositor a moment to map the window that was just shown
        std::thread::sleep(std::time::Duration::from_millis(150));
        let run = |args: &[String]| {
            std::process::Command::new("hyprctl")
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        // Lua-config Hyprland first, then the classic dispatcher
        let lua = format!("hl.dsp.focus({{ window = \"pid:{pid}\" }})");
        if !run(&["dispatch".into(), lua]) {
            run(&["dispatch".into(), "focuswindow".into(), format!("pid:{pid}")]);
        }
    });
}
