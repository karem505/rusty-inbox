import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Action pill of the reader's AI strip.
Rectangle {
    id: pill
    property string text
    property string icon: ""
    property string tip: ""
    property bool active: false
    property bool busy: false
    signal clicked()

    implicitWidth: row.implicitWidth + 24
    implicitHeight: 28
    radius: 14
    color: active ? Theme.selected : (hov.hovered ? Theme.hover : "transparent")
    border.color: active ? Theme.accent : Theme.border
    Behavior on color { ColorAnimation { duration: Theme.fast } }
    Behavior on border.color { ColorAnimation { duration: Theme.fast } }
    scale: tap.pressed ? 0.96 : 1
    Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 6
        IconImage {
            visible: pill.icon !== "" && !pill.busy
            anchors.verticalCenter: parent.verticalCenter
            width: 14; height: 14; sourceSize: Qt.size(28, 28)
            source: pill.icon !== "" ? "qrc:/qt/qml/org/rustyinbox/icons/" + pill.icon + ".svg" : ""
            color: pill.active ? Theme.accent : Theme.icon
        }
        AiDots { visible: pill.busy; anchors.verticalCenter: parent.verticalCenter }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: pill.text
            font.pixelSize: 12.5
            color: pill.active ? Theme.accent : Theme.textSubtle
        }
    }
    HoverHandler { id: hov; cursorShape: Qt.PointingHandCursor }
    TapHandler { id: tap; onTapped: pill.clicked() }
    ToolTip.visible: hov.hovered && pill.tip !== ""
    ToolTip.delay: 600
    ToolTip.text: pill.tip
}
