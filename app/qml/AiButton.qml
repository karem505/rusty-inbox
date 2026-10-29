import QtQuick
import QtQuick.Controls.impl

// Button of the AI panels: filled when `primary`, outlined otherwise, borderless when `flat`.
Rectangle {
    id: btn
    property string text
    property string icon: ""
    property bool primary: false
    property bool flat: false
    signal clicked()

    implicitWidth: row.implicitWidth + 24
    implicitHeight: 32
    radius: 8
    opacity: enabled ? 1 : 0.5
    color: primary ? (hov.hovered ? Qt.darker(Theme.accent, 1.08) : Theme.accent) : (hov.hovered ? Theme.hover : "transparent")
    border.color: primary ? Theme.accent : (flat ? "transparent" : Theme.border)
    Behavior on color { ColorAnimation { duration: Theme.fast } }
    scale: tap.pressed ? 0.96 : 1
    Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 6
        IconImage {
            visible: btn.icon !== ""
            anchors.verticalCenter: parent.verticalCenter
            width: 14; height: 14; sourceSize: Qt.size(28, 28)
            source: btn.icon !== "" ? "qrc:/qt/qml/org/rustyinbox/icons/" + btn.icon + ".svg" : ""
            color: btn.primary ? Theme.accentText : Theme.icon
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: btn.text
            font.pixelSize: 13
            font.weight: Font.DemiBold
            color: btn.primary ? Theme.accentText : Theme.text
        }
    }
    HoverHandler { id: hov; cursorShape: Qt.PointingHandCursor }
    TapHandler { id: tap; onTapped: if (btn.enabled) btn.clicked() }
}
