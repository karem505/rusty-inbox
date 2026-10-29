import QtQuick
import QtQuick.Controls.impl

// Subject suggestions under the subject field: click one to use it.
Item {
    id: root
    property var model: []
    signal picked(string subject)
    signal dismissed()

    width: parent ? parent.width : 0
    height: model.length > 0 ? flow.implicitHeight + 10 : 0
    visible: height > 0
    clip: true
    Behavior on height { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }

    Flow {
        id: flow
        y: 2
        width: parent.width
        spacing: 8
        Item {
            width: hint.implicitWidth + 4; height: 28
            Row {
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                IconImage { anchors.verticalCenter: parent.verticalCenter; width: 14; height: 14; sourceSize: Qt.size(28, 28); source: "qrc:/qt/qml/org/rustyinbox/icons/sparkle.svg"; color: Theme.accent }
                Text { id: hint; anchors.verticalCenter: parent.verticalCenter; text: qsTr("Subject ideas"); font.pixelSize: 12; color: Theme.textMuted }
            }
        }
        Repeater {
            model: root.model
            delegate: Rectangle {
                id: chip
                required property string modelData
                objectName: "subjectChip"
                height: 28
                width: Math.min(label.implicitWidth + 24, flow.width - 8)
                radius: 14
                color: ch.hovered ? Theme.selected : Theme.surface
                border.color: ch.hovered ? Theme.accent : Theme.border
                Behavior on color { ColorAnimation { duration: Theme.fast } }
                Text {
                    id: label
                    anchors.centerIn: parent
                    width: Math.min(implicitWidth, chip.width - 24)
                    text: chip.modelData
                    elide: Text.ElideRight
                    font.pixelSize: 13
                    color: Theme.text
                }
                HoverHandler { id: ch; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: root.picked(chip.modelData) }
            }
        }
        IconButton { size: 24; icon: "close"; tip: qsTr("Dismiss"); onClicked: root.dismissed() }
    }
}
