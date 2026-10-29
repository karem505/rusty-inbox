import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Daily briefing view (feature 15): the digest of unread mail as Markdown, people first. Links named
// mail:<id> open that conversation. `info` is AiInbox.briefing_json() parsed.
Rectangle {
    id: root
    color: Theme.list

    property var info: ({ state: "idle", has: false, markdown: "", day: "", error: "" })
    signal refresh()
    signal openMessage(int id)

    readonly property bool loading: info.state === "loading"
    readonly property bool failed: info.state === "error"
    readonly property bool empty: info.has === true && (info.markdown || "") === ""

    function dayLabel() {
        if (!info.day) return ""
        const d = new Date(info.day + "T12:00:00")
        const today = info.today === info.day
        const text = d.toLocaleDateString(Qt.locale(), "dddd d MMMM")
        return today ? qsTr("Today, %1").arg(text) : text
    }
    function timeLabel() {
        return info.atMs ? qsTr("written at %1").arg(new Date(info.atMs).toLocaleTimeString(Qt.locale(), "hh:mm")) : ""
    }

    Rectangle {
        id: head
        width: parent.width
        height: 44
        color: Theme.list
        z: 2
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
        Row {
            x: 16
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8
            IconImage {
                anchors.verticalCenter: parent.verticalCenter
                width: 16; height: 16
                sourceSize: Qt.size(32, 32)
                source: "qrc:/qt/qml/org/rustyinbox/icons/sparkle.svg"
                color: Theme.accent
            }
            Text { text: qsTr("Briefing"); font.pixelSize: 15; font.weight: Font.DemiBold; color: Theme.textStrong; anchors.verticalCenter: parent.verticalCenter }
            Text { text: root.dayLabel(); visible: text !== ""; font.pixelSize: 12; color: Theme.textMuted; anchors.verticalCenter: parent.verticalCenter }
            Text { text: root.timeLabel(); visible: text !== "" && !root.loading; font.pixelSize: 12; color: Theme.textMuted; anchors.verticalCenter: parent.verticalCenter }
        }
        Rectangle {
            id: refreshBtn
            objectName: "briefingRefresh"
            anchors.right: parent.right
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            width: refreshRow.width + 20
            height: 30
            radius: 8
            opacity: root.loading ? 0.5 : 1
            color: rh.hovered && !root.loading ? Theme.hover : "transparent"
            border.color: Theme.border
            Behavior on color { ColorAnimation { duration: Theme.fast } }
            Row {
                id: refreshRow
                anchors.centerIn: parent
                spacing: 6
                IconImage {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 14; height: 14
                    sourceSize: Qt.size(28, 28)
                    source: "qrc:/qt/qml/org/rustyinbox/icons/refresh.svg"
                    color: Theme.icon
                    RotationAnimator on rotation { running: root.loading; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
                }
                Text { text: qsTr("Refresh"); font.pixelSize: 13; color: Theme.text; anchors.verticalCenter: parent.verticalCenter }
            }
            HoverHandler { id: rh; cursorShape: Qt.PointingHandCursor }
            TapHandler { enabled: !root.loading; onTapped: root.refresh() }
        }
    }

    Flickable {
        id: flick
        anchors.top: head.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        contentHeight: body.height + 48
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        visible: !root.loading || (info.markdown || "") !== ""

        Text {
            id: body
            objectName: "briefingText"
            x: Math.max(24, (flick.width - width) / 2)
            y: 20
            width: Math.min(760, flick.width - 48)
            visible: (info.markdown || "") !== ""
            textFormat: Text.MarkdownText
            text: info.markdown || ""
            wrapMode: Text.Wrap
            font.pixelSize: 14
            lineHeight: 1.25
            color: Theme.text
            linkColor: Theme.accent
            opacity: root.loading ? 0.45 : 1
            Behavior on opacity { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
            onLinkActivated: (link) => {
                if (link.indexOf("mail:") === 0) root.openMessage(parseInt(link.substring(5)))
                else Qt.openUrlExternally(link)
            }
            HoverHandler { cursorShape: body.hoveredLink !== "" ? Qt.PointingHandCursor : Qt.ArrowCursor }
        }
    }

    // loading: first briefing
    Column {
        anchors.centerIn: parent
        spacing: 10
        visible: root.loading && (info.markdown || "") === ""
        BusyIndicator { anchors.horizontalCenter: parent.horizontalCenter; running: parent.visible; width: 32; height: 32 }
        Text { text: qsTr("Writing your briefing…"); font.pixelSize: 14; color: Theme.textMuted; anchors.horizontalCenter: parent.horizontalCenter }
    }
    // nothing unread
    Text {
        anchors.centerIn: parent
        visible: root.empty && !root.loading && !root.failed
        text: qsTr("No unread mail. Nothing to brief.")
        font.pixelSize: 15
        color: Theme.textMuted
    }
    // not written yet
    Column {
        anchors.centerIn: parent
        spacing: 6
        visible: !root.loading && !root.failed && !root.empty && (info.markdown || "") === ""
        Text { text: qsTr("No briefing yet"); font.pixelSize: 15; color: Theme.textMuted; anchors.horizontalCenter: parent.horizontalCenter }
        Text { text: qsTr("It is written once a day. Press Refresh to write one now."); font.pixelSize: 12; color: Theme.textMuted; anchors.horizontalCenter: parent.horizontalCenter }
    }
    // failed: the provider's own message, and a way to try again
    Rectangle {
        objectName: "briefingError"
        visible: root.failed
        anchors.horizontalCenter: parent.horizontalCenter
        y: head.height + 16
        width: Math.min(560, parent.width - 48)
        height: errCol.height + 24
        radius: 10
        color: Theme.surface
        border.color: Theme.border
        Column {
            id: errCol
            x: 14
            y: 12
            width: parent.width - 28
            spacing: 8
            Text { text: qsTr("The briefing could not be written"); font.pixelSize: 14; font.weight: Font.DemiBold; color: Theme.danger }
            Text { width: parent.width; text: root.info.error || ""; wrapMode: Text.Wrap; font.pixelSize: 13; color: Theme.text }
            Text {
                text: qsTr("Try again")
                font.pixelSize: 13
                color: Theme.accent
                HoverHandler { cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: root.refresh() }
            }
        }
    }
}
