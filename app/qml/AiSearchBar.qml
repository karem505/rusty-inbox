import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Search field of the message list. Words and `from:` / `has:attachments` / `since:` tokens search at
// once. The sparkle button (or Enter on text that starts with "?") asks the model to turn a question
// into such a query; the query replaces the text, so it can be edited, and runs.
Rectangle {
    id: root
    property alias text: input.text
    property bool askAvailable: false
    property bool busy: false
    property string error: ""
    readonly property bool inputFocused: input.activeFocus
    signal submit(string text, bool ask)
    signal closed()

    function focusInput() { input.forceActiveFocus(); input.selectAll() }

    color: Theme.list
    implicitHeight: 48 + (note.visible ? note.height + 6 : 0)
    clip: true

    Rectangle {
        id: field
        x: 10
        y: 8
        width: parent.width - 20
        height: 32
        radius: 8
        color: Theme.surface
        border.color: input.activeFocus ? Theme.accent : Theme.border
        border.width: input.activeFocus ? 1.5 : 1
        Behavior on border.color { ColorAnimation { duration: Theme.fast } }

        IconImage {
            id: lens
            x: 9
            anchors.verticalCenter: parent.verticalCenter
            width: 15; height: 15
            sourceSize: Qt.size(30, 30)
            source: "qrc:/qt/qml/org/rustyinbox/icons/search.svg"
            color: Theme.textMuted
        }
        TextField {
            id: input
            objectName: "searchInput"
            anchors.left: lens.right
            anchors.leftMargin: 4
            anchors.right: tools.left
            anchors.verticalCenter: parent.verticalCenter
            background: Item {}
            color: Theme.text
            placeholderTextColor: Theme.textMuted
            font.pixelSize: 14
            selectByMouse: true
            enabled: !root.busy
            placeholderText: root.askAvailable ? qsTr("Search, or ask in plain words with ?") : qsTr("Search mail")
            onAccepted: root.submit(text, text.trim().startsWith("?"))
            Keys.onEscapePressed: root.closed()
        }
        Row {
            id: tools
            anchors.right: parent.right
            anchors.rightMargin: 4
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            Text {
                id: asking
                visible: root.busy
                anchors.verticalCenter: parent.verticalCenter
                rightPadding: 6
                text: qsTr("Asking…")
                font.pixelSize: 12
                color: Theme.accent
                SequentialAnimation on opacity {
                    running: root.busy
                    loops: Animation.Infinite
                    NumberAnimation { from: 1; to: 0.35; duration: 600; easing.type: Easing.InOutSine }
                    NumberAnimation { from: 0.35; to: 1; duration: 600; easing.type: Easing.InOutSine }
                }
            }
            IconButton {
                objectName: "askButton"
                size: 26
                visible: root.askAvailable
                enabled: !root.busy
                icon: "sparkle"
                tip: qsTr("Ask in plain words (Enter on text that starts with ?)")
                onClicked: root.submit(input.text, true)
            }
            IconButton { size: 26; icon: "close"; tip: qsTr("Close search (Esc)"); onClicked: root.closed() }
        }
    }
    Text {
        id: note
        x: 14
        y: 46
        width: parent.width - 28
        visible: text !== ""
        text: root.error
        wrapMode: Text.WordWrap
        font.pixelSize: 12
        color: Theme.danger
    }
}
