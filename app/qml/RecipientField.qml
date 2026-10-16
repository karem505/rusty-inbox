import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// One recipient line (To / Cc / Bcc): addresses become chips; typing suggests people you have
// mail with. Enter, Tab, comma or semicolon turns the text into a chip; Backspace on an empty
// field removes the last one; pasting "a@x.com, Bob <b@y.org>" adds them all.
Item {
    id: root
    property string label
    property var backend
    property var addresses: []          // [{name, address}]
    property bool showLabelLink: false
    signal edited()

    implicitHeight: Math.max(40, flow.implicitHeight + 12)

    function valid(a) { return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(a) }
    function parseOne(t) {
        t = t.trim().replace(/^[,;]+|[,;]+$/g, "").trim()
        if (!t) return null
        const m = t.match(/^\s*"?([^"<]*?)"?\s*<([^>]+)>\s*$/)
        if (m) return { name: m[1].trim() || null, address: m[2].trim() }
        return { name: null, address: t }
    }
    function add(entry) {
        if (!entry || !entry.address) return
        const key = entry.address.toLowerCase()
        if (addresses.some(a => a.address.toLowerCase() === key)) return
        addresses = addresses.concat([{ name: entry.name || null, address: entry.address }])
        edited()
    }
    function commit() {
        const parts = input.text.split(/[,;\n]+/)
        for (const p of parts) add(parseOne(p))
        input.text = ""
        suggestions.close()
    }
    function removeAt(i) {
        const a = addresses.slice()
        a.splice(i, 1)
        addresses = a
        edited()
    }
    // the pending text counts too (sending with an address still typed in the field)
    function finish() { if (input.text.trim()) commit() }
    function focusInput() { input.forceActiveFocus() }

    Text {
        id: lab
        x: 0
        y: 12
        width: 44
        text: root.label
        font.pixelSize: 13
        color: Theme.textMuted
    }
    Flow {
        id: flow
        anchors.left: lab.right
        anchors.right: parent.right
        y: 6
        spacing: 6
        Repeater {
            model: root.addresses
            delegate: Rectangle {
                required property var modelData
                required property int index
                readonly property bool ok: root.valid(modelData.address)
                height: 28
                width: chipRow.implicitWidth + 16
                radius: 14
                color: ok ? Theme.selected : Qt.rgba(1, 0.3, 0.3, 0.16)
                border.color: ok ? "transparent" : Theme.danger
                Row {
                    id: chipRow
                    anchors.centerIn: parent
                    spacing: 6
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.name || modelData.address
                        font.pixelSize: 13
                        color: Theme.text
                        ToolTip.visible: chipHover.hovered && !!modelData.name
                        ToolTip.text: modelData.address
                        HoverHandler { id: chipHover }
                    }
                    IconImage {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 12; height: 12
                        sourceSize: Qt.size(24, 24)
                        source: "qrc:/qt/qml/org/rustyinbox/icons/close.svg"
                        color: Theme.textMuted
                        TapHandler { onTapped: root.removeAt(index) }
                        HoverHandler { cursorShape: Qt.PointingHandCursor }
                    }
                }
            }
        }
        TextInput {
            id: input
            width: 180
            height: 28
            verticalAlignment: TextInput.AlignVCenter
            font.pixelSize: 14
            color: Theme.text
            selectByMouse: true
            clip: true
            Text {
                visible: !input.text && root.addresses.length === 0
                anchors.verticalCenter: parent.verticalCenter
                text: qsTr("Name or email")
                color: Theme.textMuted
                font.pixelSize: 14
            }
            onTextEdited: {
                if (/[,;]$/.test(text)) { root.commit(); return }
                const q = text.trim()
                if (q.length < 1) { suggestions.close(); return }
                const list = JSON.parse(root.backend.contacts_json(q))
                    .filter(c => !root.addresses.some(a => a.address.toLowerCase() === c.address))
                sugModel.clear()
                for (const c of list) sugModel.append({ name: c.name, address: c.address })
                sugList.currentIndex = 0
                if (sugModel.count) suggestions.open(); else suggestions.close()
            }
            Keys.onPressed: (e) => {
                if (suggestions.opened && (e.key === Qt.Key_Down || e.key === Qt.Key_Up)) {
                    sugList.currentIndex = Math.max(0, Math.min(sugModel.count - 1, sugList.currentIndex + (e.key === Qt.Key_Down ? 1 : -1)))
                    e.accepted = true
                } else if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter || e.key === Qt.Key_Tab) {
                    if (suggestions.opened && sugModel.count) {
                        const c = sugModel.get(sugList.currentIndex)
                        root.add({ name: c.name, address: c.address })
                        input.text = ""
                        suggestions.close()
                        e.accepted = true
                    } else if (input.text.trim()) {
                        root.commit()
                        e.accepted = true
                    }
                } else if (e.key === Qt.Key_Backspace && !input.text && root.addresses.length) {
                    root.removeAt(root.addresses.length - 1)
                    e.accepted = true
                } else if (e.key === Qt.Key_Escape && suggestions.opened) {
                    suggestions.close()
                    e.accepted = true
                }
            }
            onActiveFocusChanged: if (!activeFocus) Qt.callLater(() => { if (!suggestions.opened) root.finish() })
        }
    }
    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
    // clicking the empty part of the line puts the caret in the field
    TapHandler { onTapped: input.forceActiveFocus() }

    ListModel { id: sugModel }
    Popup {
        id: suggestions
        y: root.height
        x: lab.width
        width: Math.min(420, root.width - lab.width)
        padding: 4
        focus: false
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent
        background: Rectangle { radius: 10; color: Theme.surface; border.color: Theme.border }
        contentItem: ListView {
            id: sugList
            implicitHeight: contentHeight
            model: sugModel
            interactive: false
            delegate: Rectangle {
                required property string name
                required property string address
                required property int index
                width: ListView.view.width
                height: 44
                radius: 8
                color: ListView.isCurrentItem || h.hovered ? Theme.hover : "transparent"
                Rectangle {
                    id: av
                    x: 8
                    anchors.verticalCenter: parent.verticalCenter
                    width: 28; height: 28; radius: 14
                    color: Theme.avatarFor(address)
                    Text {
                        anchors.centerIn: parent
                        text: (name || address).charAt(0).toUpperCase()
                        color: Theme.dark ? Theme.window : "white"
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                    }
                }
                Column {
                    anchors.left: av.right
                    anchors.leftMargin: 10
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    Text { width: parent.width; text: name || address; elide: Text.ElideRight; font.pixelSize: 13; color: Theme.text }
                    Text { width: parent.width; visible: !!name; text: address; elide: Text.ElideRight; font.pixelSize: 11; color: Theme.textMuted }
                }
                HoverHandler { id: h; cursorShape: Qt.PointingHandCursor }
                TapHandler {
                    onTapped: {
                        root.add({ name: name, address: address })
                        input.text = ""
                        suggestions.close()
                        input.forceActiveFocus()
                    }
                }
            }
        }
    }
}
