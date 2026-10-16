import QtQuick

// Bottom-centre message with an Undo button; slides up, hides itself after `timeout`.
Rectangle {
    id: root
    property string message
    property bool canUndo: false
    property int timeout: 5000
    property bool shown: false
    signal undo()

    function show(msg, undoable) {
        message = msg
        canUndo = undoable
        shown = true
        hideTimer.restart()
    }

    visible: opacity > 0
    opacity: shown ? 1 : 0
    property int lift: 0                // keeps clear of a bottom bar (the composer's Send row)
    anchors.bottomMargin: (shown ? 24 : 8) + lift
    Behavior on opacity { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
    Behavior on anchors.bottomMargin { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
    height: 42
    width: row.implicitWidth + 32
    radius: 10
    color: Theme.toast
    border.color: Theme.border
    border.width: Theme.dark ? 1 : 0

    Timer { id: hideTimer; interval: root.timeout; onTriggered: root.shown = false }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 18
        Text { text: root.message; color: Theme.toastText; font.pixelSize: 14; anchors.verticalCenter: parent.verticalCenter }
        Text {
            visible: root.canUndo
            text: qsTr("Undo")
            color: Theme.toastAction
            font.pixelSize: 14
            font.weight: Font.DemiBold
            anchors.verticalCenter: parent.verticalCenter
            HoverHandler { cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: { root.shown = false; root.undo() } }
        }
    }
}
