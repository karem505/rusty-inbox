import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Snooze presets. Emits `picked(untilMs, label)`. Each row: preset name left, time right.
Menu {
    id: menu
    signal picked(double untilMs, string label)

    function at(daysAhead, hour) {
        const d = new Date()
        d.setDate(d.getDate() + daysAhead)
        d.setHours(hour, 0, 0, 0)
        return d.getTime()
    }
    function laterToday() {
        return Date.now() + 3 * 3600 * 1000
    }
    function tonight() {
        const t = at(0, 19)
        return t > Date.now() + 30 * 60 * 1000 ? t : at(1, 19)
    }
    function weekend() {
        const d = new Date()
        const toSat = (6 - d.getDay() + 7) % 7 || 7
        return at(toSat, 9)
    }
    function nextWeek() {
        const d = new Date()
        const toMon = (8 - d.getDay()) % 7 || 7
        return at(toMon, 8)
    }
    function label(ms) {
        return new Date(ms).toLocaleString(Qt.locale(), "ddd d MMM, hh:mm")
    }

    readonly property var presets: [
        { name: qsTr("Later today"), icon: "clock", at: () => laterToday() },
        { name: qsTr("This evening"), icon: "clock", at: () => tonight() },
        { name: qsTr("Tomorrow"), icon: "calendar", at: () => at(1, 8) },
        { name: qsTr("This weekend"), icon: "calendar", at: () => weekend() },
        { name: qsTr("Next week"), icon: "calendar", at: () => nextWeek() }
    ]

    width: 360
    padding: 6
    background: Rectangle {
        radius: 12
        color: Theme.surface
        border.color: Theme.border
    }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.96; to: 1; duration: Theme.normal; easing.type: Theme.easing }
    }
    exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: Theme.fast } }

    Repeater {
        model: menu.presets
        delegate: MenuItem {
            id: mi
            required property var modelData
            readonly property double when: modelData.at()
            implicitHeight: 40
            onTriggered: menu.picked(when, modelData.name + "   " + menu.label(when))
            background: Rectangle { radius: 8; color: mi.highlighted ? Theme.hover : "transparent" }
            contentItem: Item {
                IconImage {
                    id: ic
                    anchors.verticalCenter: parent.verticalCenter
                    width: 16; height: 16
                    sourceSize: Qt.size(32, 32)
                    source: "qrc:/qt/qml/org/rustyinbox/icons/" + mi.modelData.icon + ".svg"
                    color: Theme.icon
                }
                Text {
                    anchors.left: ic.right
                    anchors.leftMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    text: mi.modelData.name
                    font.pixelSize: 14
                    color: Theme.text
                }
                Text {
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    text: menu.label(mi.when)
                    font.pixelSize: 12
                    color: Theme.textMuted
                }
            }
        }
    }
}
