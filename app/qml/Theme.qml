pragma Singleton
import QtQuick

// Every colour and the font the UI uses. Filled from the backend (`theme_json`, Omarchy's current
// theme when present) and updated live on `theme_changed`. Defaults are the built-in light palette.
QtObject {
    id: t
    property bool dark: false
    property string font: ""
    property color window: "#ffffff"
    property color list: "#ffffff"
    property color sidebar: "#eceef2"
    property color surface: "#f4f5f8"
    property color hover: "#f2f3f6"
    property color selected: "#e3ebfd"
    property color navSelected: "#d9dde6"
    property color border: "#ebecef"
    property color section: "#fafbfc"
    property color text: "#1d1d22"
    property color textStrong: "#1d1d22"
    property color textMuted: "#8a8d96"
    property color textSubtle: "#5b5f6b"
    property color icon: "#5b5f6b"
    property color accent: "#2f6bed"
    property color accentText: "#ffffff"
    property color danger: "#d64545"
    property color toast: "#2b2d33"
    property color toastText: "#ffffff"
    property color toastAction: "#8fb4ff"
    property var avatars: ["#d64545", "#e07b39", "#c99a16", "#3f9d5a", "#2b9cb0", "#2f6bed", "#a557c9"]

    // motion: short and decelerating, so the UI feels instant but not abrupt
    readonly property int fast: 110
    readonly property int normal: 170
    readonly property int easing: Easing.OutCubic

    function avatarFor(key) {
        let h = 0
        const s = String(key || "")
        for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0
        return avatars[h % avatars.length]
    }

    function apply(p) {
        if (!p) return
        for (const k in p) {
            if (k === "source") continue
            if (k === "font") { t.font = p.font || ""; continue }
            if (t[k] !== undefined) t[k] = p[k]
        }
    }
}
