import QtQuick
import QtQuick.Controls.impl

// Round-cornered check box: empty, checked, or partly checked (some of a group selected).
Rectangle {
    id: root
    property bool checked: false
    property bool partial: false
    property int boxSize: 18
    signal toggled()

    width: boxSize
    height: boxSize
    radius: 5
    color: checked || partial ? Theme.accent : "transparent"
    border.width: checked || partial ? 0 : 1.6
    border.color: hover.hovered ? Theme.accent : Theme.textMuted
    Behavior on color { ColorAnimation { duration: Theme.fast } }
    scale: tap.pressed ? 0.88 : 1
    Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }

    IconImage {
        anchors.centerIn: parent
        visible: root.checked
        width: root.boxSize - 2; height: width
        sourceSize: Qt.size(width * 2, height * 2)
        source: "qrc:/qt/qml/org/rustyinbox/icons/done.svg"
        color: Theme.accentText
    }
    Rectangle {
        anchors.centerIn: parent
        visible: root.partial && !root.checked
        width: root.boxSize / 2; height: 2; radius: 1
        color: Theme.accentText
    }
    HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
    TapHandler { id: tap; onTapped: root.toggled() }
}
