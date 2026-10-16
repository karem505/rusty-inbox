import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Square icon button with tooltip. `icon` is a name from icons/; it is recoloured to the theme.
Rectangle {
    id: root
    property string icon
    property string tip
    property bool active: false
    property int size: 30
    property color tint: active ? Theme.accent : Theme.icon
    signal clicked()

    width: size
    height: size
    radius: 7
    color: active ? Theme.selected : (hover.hovered ? Theme.hover : "transparent")
    Behavior on color { ColorAnimation { duration: Theme.fast } }
    scale: tap.pressed ? 0.9 : 1
    Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }

    IconImage {
        anchors.centerIn: parent
        width: Math.round(root.size * 0.6)
        height: width
        sourceSize: Qt.size(width * 2, height * 2)
        source: "qrc:/qt/qml/org/rustyinbox/icons/" + root.icon + ".svg"
        color: root.tint
        opacity: root.enabled ? 1 : 0.35
    }
    // a click hides the tooltip until the pointer leaves (it would cover the menu it opened)
    property bool tipBlocked: false
    HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor; onHoveredChanged: if (!hovered) root.tipBlocked = false }
    TapHandler { id: tap; onTapped: { root.tipBlocked = true; root.clicked() } }
    ToolTip.visible: hover.hovered && tip !== "" && !tipBlocked
    ToolTip.delay: 600
    ToolTip.text: tip
}
