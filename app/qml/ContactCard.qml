import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// A person's card, opened from a name or address in the reader (From, To, Cc, Bcc): who they
// are, how much mail you have with them, their latest conversations, and what to do next.
Popup {
    id: root
    property var backend
    property var contact: null          // engine::contact_of
    property string fallbackName: ""    // the name the message showed, until the store knows better
    signal writeTo(string address, string name)
    signal showAll(string address)
    signal openConversation(string address, int threadId)
    signal notice(string text)

    function show(address, name) {
        fallbackName = name || ""
        contact = JSON.parse(backend.contact_json(address))
        open()
    }
    readonly property string address: contact ? contact.address : ""
    readonly property string displayName: contact && contact.name ? contact.name : (fallbackName && fallbackName.indexOf("@") < 0 ? fallbackName : "")
    readonly property var threads: contact ? contact.threads : []

    function initials(name) {
        const parts = (name || "?").replace(/[^\p{L}\p{N} ]/gu, "").trim().split(/\s+/)
        return ((parts[0] || "?")[0] + (parts.length > 1 ? parts[parts.length - 1][0] : "")).toUpperCase()
    }
    function when(ms) {
        const d = new Date(ms)
        const now = new Date()
        if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString(Qt.locale(), "hh:mm")
        if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString(Qt.locale(), "d MMM")
        return d.toLocaleDateString(Qt.locale(), "d MMM yyyy")
    }
    function summary() {
        if (!contact) return ""
        if (contact.isMe) return qsTr("Your account")
        const parts = []
        if (contact.fromThem > 0) parts.push(contact.fromThem === 1 ? qsTr("1 message from them") : qsTr("%1 messages from them").arg(contact.fromThem))
        if (contact.toThem > 0) parts.push(contact.toThem === 1 ? qsTr("1 message to them") : qsTr("%1 messages to them").arg(contact.toThem))
        if (!parts.length) return qsTr("No other mail with this address")
        return parts.join(" · ")
    }
    function copyAddress() {
        clip.text = address
        clip.selectAll()
        clip.copy()
        notice(qsTr("Copied %1").arg(address))
    }

    anchors.centerIn: Overlay.overlay
    width: Math.min(480, parent ? parent.width - 48 : 480)
    padding: 0
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: Theme.dark ? "#80000000" : "#33000000" }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.normal; easing.type: Theme.easing }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.normal; easing.type: Theme.easing }
    }
    exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: Theme.fast } }

    background: Rectangle {
        radius: 14
        color: Theme.window
        border.color: Theme.border
    }

    // the clipboard is reached through a text field
    TextEdit { id: clip; visible: false }

    contentItem: Column {
        objectName: "contactCard"
        spacing: 0

        // ---- who
        Item {
            width: parent.width
            height: 96
            Rectangle {
                id: avatar
                x: 22
                anchors.verticalCenter: parent.verticalCenter
                width: 56
                height: 56
                radius: 28
                color: Theme.avatarFor(root.address)
                Text {
                    anchors.centerIn: parent
                    text: root.initials(root.displayName || root.address)
                    color: "#ffffff"
                    font.pixelSize: 21
                    font.weight: Font.DemiBold
                }
            }
            Column {
                anchors.left: avatar.right
                anchors.leftMargin: 16
                anchors.right: closeBtn.left
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 3
                Row {
                    spacing: 8
                    width: parent.width
                    Text {
                        text: root.displayName || root.address
                        color: Theme.textStrong
                        font.pixelSize: 18
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                        width: Math.min(implicitWidth, parent.width - (meBadge.visible ? meBadge.width + 8 : 0))
                    }
                    Rectangle {
                        id: meBadge
                        visible: !!(root.contact && root.contact.isMe)
                        anchors.verticalCenter: parent.verticalCenter
                        width: meText.implicitWidth + 12
                        height: 20
                        radius: 10
                        color: Theme.selected
                        Text { id: meText; anchors.centerIn: parent; text: qsTr("You"); color: Theme.accent; font.pixelSize: 11; font.weight: Font.DemiBold }
                    }
                }
                TextEdit {
                    objectName: "contactAddress"
                    visible: root.displayName !== ""
                    width: parent.width
                    text: root.address
                    readOnly: true
                    selectByMouse: true
                    color: Theme.textMuted
                    selectionColor: Theme.accent
                    selectedTextColor: Theme.accentText
                    font.pixelSize: 13
                    wrapMode: TextEdit.WrapAnywhere
                }
                Text {
                    width: parent.width
                    text: root.summary()
                    color: Theme.textSubtle
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }
            }
            IconButton {
                id: closeBtn
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.top: parent.top
                anchors.topMargin: 12
                icon: "close"
                tip: qsTr("Close (Esc)")
                onClicked: root.close()
            }
        }

        // ---- actions
        Flow {
            x: 22
            width: parent.width - 44
            spacing: 8
            bottomPadding: 16

            component Action: Rectangle {
                id: act
                property string icon
                property string label
                property bool primary: false
                signal clicked()
                height: 34
                width: actRow.implicitWidth + 24
                radius: 8
                color: primary ? (actHover.hovered ? Qt.darker(Theme.accent, 1.08) : Theme.accent) : (actHover.hovered ? Theme.hover : Theme.surface)
                border.color: primary ? "transparent" : Theme.border
                Behavior on color { ColorAnimation { duration: Theme.fast } }
                scale: actTap.pressed ? 0.97 : 1
                Row {
                    id: actRow
                    anchors.centerIn: parent
                    spacing: 7
                    IconImage {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 15; height: 15; sourceSize: Qt.size(30, 30)
                        source: "qrc:/qt/qml/org/rustyinbox/icons/" + act.icon + ".svg"
                        color: act.primary ? Theme.accentText : Theme.icon
                    }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: act.label; font.pixelSize: 13; color: act.primary ? Theme.accentText : Theme.text }
                }
                HoverHandler { id: actHover; cursorShape: Qt.PointingHandCursor }
                TapHandler { id: actTap; onTapped: act.clicked() }
            }

            Action {
                objectName: "contactWrite"
                icon: "compose"
                label: qsTr("Write email")
                primary: true
                onClicked: { root.close(); root.writeTo(root.address, root.displayName) }
            }
            Action {
                icon: "copy"
                label: qsTr("Copy address")
                onClicked: root.copyAddress()
            }
            Action {
                visible: root.threads.length > 0
                icon: "people"
                label: qsTr("All conversations")
                onClicked: { root.close(); root.showAll(root.address) }
            }
        }

        // ---- latest conversations
        Rectangle { width: parent.width; height: 1; color: Theme.border; visible: root.threads.length > 0 }
        Text {
            visible: root.threads.length > 0
            x: 22
            topPadding: 12
            bottomPadding: 4
            text: qsTr("Recent conversations")
            color: Theme.textMuted
            font.pixelSize: 11
            font.weight: Font.DemiBold
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 0.4
        }
        ListView {
            id: recent
            visible: root.threads.length > 0
            width: parent.width
            height: Math.min(contentHeight, 6 * 52)
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.threads
            ScrollBar.vertical: ScrollBar { policy: recent.contentHeight > recent.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }
            delegate: Rectangle {
                id: trow
                required property var modelData
                width: recent.width
                height: 52
                color: rowHover.hovered ? Theme.hover : "transparent"
                Column {
                    x: 22
                    width: parent.width - 44 - dateText.implicitWidth - 12
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    Text {
                        width: parent.width
                        text: trow.modelData.subject || qsTr("(no subject)")
                        color: Theme.text
                        font.pixelSize: 13
                        font.weight: trow.modelData.unread ? Font.DemiBold : Font.Normal
                        elide: Text.ElideRight
                    }
                    Text {
                        width: parent.width
                        text: trow.modelData.preview
                        color: Theme.textMuted
                        font.pixelSize: 12
                        elide: Text.ElideRight
                    }
                }
                Text {
                    id: dateText
                    anchors.right: parent.right
                    anchors.rightMargin: 22
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.when(trow.modelData.date)
                    color: Theme.textMuted
                    font.pixelSize: 12
                }
                HoverHandler { id: rowHover; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: { root.close(); root.openConversation(root.address, trow.modelData.id) } }
            }
        }
        Item { width: 1; height: root.threads.length > 0 ? 8 : 4 }
    }
}
