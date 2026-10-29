import QtQuick

// Three pulsing dots: "the assistant is working".
Row {
    id: dots
    property bool running: true
    spacing: 4
    Repeater {
        model: 3
        delegate: Rectangle {
            required property int index
            width: 5; height: 5; radius: 2.5
            color: Theme.accent
            opacity: 0.3
            SequentialAnimation on opacity {
                running: dots.running && dots.visible
                loops: Animation.Infinite
                PauseAnimation { duration: index * 140 }
                NumberAnimation { to: 1; duration: 280; easing.type: Theme.easing }
                NumberAnimation { to: 0.3; duration: 420; easing.type: Theme.easing }
                PauseAnimation { duration: (2 - index) * 140 }
            }
        }
    }
}
