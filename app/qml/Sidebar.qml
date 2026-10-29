import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Navigation: smart inbox + categories, system lists, then each account with its own folders.
Rectangle {
    id: root
    color: Theme.sidebar

    property var model: ({ inbox: 0, pinned: 0, snoozed: 0, drafts: 0, categories: [], accounts: [] })
    property var spec: ({ list: "inbox" })
    signal select(var spec)
    signal openSettings(string page)
    property string updateVersion: ""
    signal compose()
    // AI(C2): which organizing features are on ({needsReply, followUps, briefing, filter}) and the quiet error status
    property var aiFeat: ({})
    property string aiStatus: ""

    property var expanded: ({})
    // unified lists unfolded into their accounts (list name -> true)
    property var listOpen: ({})
    readonly property bool multiAccount: root.model.accounts.length > 1
    function listExpanded(list) { return multiAccount && listOpen[list] === true }
    // First tap shows the list for all accounts and unfolds its accounts; tapping it again while it
    // is shown folds them away. Picking an account below shows only that account.
    function tapList(target) {
        const o = Object.assign({}, listOpen)
        if (root.isCurrent(target)) o[target.list] = !o[target.list]
        else o[target.list] = true
        listOpen = o
        if (!root.isCurrent(target)) root.select(target)
    }
    function accountLabel(a) { return a.name && a.name.length ? a.name : a.email }
    property string appName: "Rusty Inbox"

    readonly property var categoryInfo: ({
        personal: { label: qsTr("People"), icon: "people" },
        notifications: { label: qsTr("Notifications"), icon: "bell" },
        newsletters: { label: qsTr("Newsletters"), icon: "news" },
        invitations: { label: qsTr("Invitations"), icon: "calendar" }
    })

    function key(s) {
        return [s.list || "inbox", s.account || "", s.category || "", s.folder || ""].join("|")
    }
    function isCurrent(s) { return key(s) === key(root.spec) }

    component NavItem: Rectangle {
        id: item
        property var target
        property string label
        property string icon
        property int count: 0
        property bool countIsUnread: true
        property string note: ""       // short text instead of a count (e.g. "Update")
        property int indent: 0
        width: parent ? parent.width : 0
        height: 32
        radius: 8
        color: root.isCurrent(target) ? Theme.navSelected : (hover.hovered ? Theme.hover : "transparent")
        Behavior on color { ColorAnimation { duration: Theme.fast } }
        property bool expandable: false
        property bool open: false
        property var action: null     // custom tap (e.g. Settings) instead of selecting a list
        property string tip: ""
        HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
        ToolTip.visible: hover.hovered && item.tip !== ""
        ToolTip.delay: 600
        ToolTip.text: item.tip
        TapHandler {
            onTapped: {
                if (item.action) item.action()
                else if (item.expandable) root.tapList(item.target)
                else root.select(item.target)
            }
        }
        IconImage {
            visible: item.expandable && (hover.hovered || item.open)
            anchors.right: badge.visible ? badge.left : parent.right
            anchors.rightMargin: badge.visible ? 6 : 10
            anchors.verticalCenter: parent.verticalCenter
            width: 12; height: 12
            sourceSize: Qt.size(24, 24)
            source: "qrc:/qt/qml/org/rustyinbox/icons/chevron.svg"
            color: Theme.textMuted
            rotation: item.open ? 90 : 0
            Behavior on rotation { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
        }

        IconImage {
            id: ic
            color: root.isCurrent(item.target) ? Theme.accent : Theme.icon
            x: 10 + item.indent
            anchors.verticalCenter: parent.verticalCenter
            width: 18; height: 18
            sourceSize: Qt.size(36, 36)
            source: "qrc:/qt/qml/org/rustyinbox/icons/" + item.icon + ".svg"
        }
        Text {
            anchors.left: ic.right
            anchors.leftMargin: 10
            anchors.right: badge.left
            anchors.rightMargin: 6
            anchors.verticalCenter: parent.verticalCenter
            text: item.label
            elide: Text.ElideRight
            font.pixelSize: 14
            font.weight: root.isCurrent(item.target) ? Font.DemiBold : Font.Normal
            color: Theme.text
        }
        Text {
            id: badge
            anchors.right: parent.right
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            visible: item.count > 0 || item.note !== ""
            text: item.note !== "" ? item.note : item.count
            font.pixelSize: 12
            font.weight: item.countIsUnread ? Font.DemiBold : Font.Normal
            color: item.countIsUnread ? Theme.accent : Theme.textMuted
        }
    }

    // A unified list (all accounts) that unfolds into one entry per account.
    component UnifiedList: Column {
        id: ul
        property string list
        property string label
        property string icon
        property int count: 0
        property bool countIsUnread: true
        readonly property bool open: root.listExpanded(list)
        width: parent ? parent.width : 0
        spacing: 1
        NavItem {
            target: ({ list: ul.list })
            label: ul.label
            icon: ul.icon
            count: ul.count
            countIsUnread: ul.countIsUnread
            expandable: root.multiAccount
            open: ul.open
        }
        Item {
            width: parent.width
            height: ul.open ? accCol.height : 0
            clip: true
            Behavior on height { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
            Column {
                id: accCol
                width: parent.width
                spacing: 1
                Repeater {
                    model: root.model.accounts
                    delegate: NavItem {
                        required property var modelData
                        target: ({ list: ul.list, account: modelData.email })
                        label: root.accountLabel(modelData)
                        icon: "account"
                        indent: 14
                        count: ul.list === "inbox" ? modelData.unread : 0
                    }
                }
            }
        }
    }

    component Header: Text {
        width: parent ? parent.width : 0
        leftPadding: 10
        topPadding: 14
        bottomPadding: 4
        font.pixelSize: 11
        font.weight: Font.DemiBold
        font.capitalization: Font.AllUppercase
        color: Theme.textMuted
    }

    Rectangle {
        id: footer
        anchors.bottom: parent.bottom
        width: parent.width
        height: 48
        color: "transparent"
        Rectangle { width: parent.width; height: 1; color: Theme.border }
        NavItem {
            x: 10
            width: parent.width - 20
            anchors.verticalCenter: parent.verticalCenter
            target: ({ list: "__settings" })
            label: qsTr("Settings")
            icon: "settings"
            note: root.updateVersion !== "" ? qsTr("Update %1").arg(root.updateVersion) : ""
            action: () => root.openSettings(root.updateVersion !== "" ? "updates" : "general")
        }
    }

    Flickable {
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: footer.top
        anchors.margins: 10
        contentHeight: col.height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}

        Column {
            id: col
            width: parent.width
            spacing: 1

            Row {
                leftPadding: 6
                bottomPadding: 10
                spacing: 9
                Image {
                    width: 26; height: 26
                    sourceSize: Qt.size(52, 52)
                    source: "qrc:/qt/qml/org/rustyinbox/icons/app.png"
                    anchors.verticalCenter: parent.verticalCenter
                    smooth: true
                }
                Text {
                    text: root.appName
                    font.pixelSize: 19
                    font.weight: Font.DemiBold
                    color: Theme.textStrong
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            // compose
            Rectangle {
                id: composeBtn
                objectName: "composeButton"
                width: col.width
                height: 40
                radius: 10
                color: ch.hovered ? Qt.darker(Theme.accent, 1.08) : Theme.accent
                scale: ct.pressed ? 0.97 : 1
                Behavior on scale { NumberAnimation { duration: Theme.fast } }
                Behavior on color { ColorAnimation { duration: Theme.fast } }
                Row {
                    anchors.centerIn: parent
                    spacing: 8
                    IconImage { anchors.verticalCenter: parent.verticalCenter; width: 17; height: 17; sourceSize: Qt.size(34, 34); source: "qrc:/qt/qml/org/rustyinbox/icons/compose.svg"; color: Theme.accentText }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Compose"); font.pixelSize: 14; font.weight: Font.DemiBold; color: Theme.accentText }
                }
                HoverHandler { id: ch; cursorShape: Qt.PointingHandCursor }
                TapHandler { id: ct; onTapped: root.compose() }
                ToolTip.visible: ch.hovered
                ToolTip.delay: 600
                ToolTip.text: qsTr("New message (C)")
            }
            Item { width: 1; height: 8 }
            UnifiedList { list: "inbox"; label: qsTr("Smart Inbox"); icon: "inbox"; count: root.model.inbox }
            Repeater {
                model: root.model.categories
                delegate: NavItem {
                    required property var modelData
                    target: ({ list: "inbox", category: modelData.key })
                    label: root.categoryInfo[modelData.key] ? root.categoryInfo[modelData.key].label : modelData.key
                    icon: root.categoryInfo[modelData.key] ? root.categoryInfo[modelData.key].icon : "inbox"
                    count: modelData.unread
                    indent: 14
                }
            }
            // AI(C2): organizing lists, shown only when their feature is on
            NavItem {
                objectName: "needsReplyEntry"
                visible: root.aiFeat.needsReply === true
                target: ({ list: "needsReply" })
                label: qsTr("Needs reply")
                icon: "reply"
                count: root.model.needsReply || 0
                countIsUnread: false
                tip: qsTr("Mail that expects an answer from you")
            }
            NavItem {
                objectName: "followUpEntry"
                visible: root.aiFeat.followUps === true
                target: ({ list: "followUp" })
                label: qsTr("Follow up")
                icon: "follow-up"
                count: root.model.followUp || 0
                countIsUnread: false
                tip: qsTr("Mail you sent that got no answer yet")
            }
            NavItem {
                objectName: "briefingEntry"
                visible: root.aiFeat.briefing === true
                target: ({ list: "briefing" })
                label: qsTr("Briefing")
                icon: "sparkle"
            }
            Text {
                objectName: "aiStatus"
                visible: root.aiStatus !== "" && (root.aiFeat.needsReply === true || root.aiFeat.followUps === true || root.aiFeat.briefing === true || root.aiFeat.filter === true)
                width: col.width
                leftPadding: 10
                topPadding: 2
                bottomPadding: 2
                text: qsTr("AI paused, will retry")
                elide: Text.ElideRight
                font.pixelSize: 12
                color: Theme.textMuted
                HoverHandler { id: statusHover }
                ToolTip.visible: statusHover.hovered
                ToolTip.delay: 300
                ToolTip.text: root.aiStatus
            }
            UnifiedList { list: "pinned"; label: qsTr("Pinned"); icon: "pin"; count: root.model.pinned; countIsUnread: false }
            UnifiedList { list: "snoozed"; label: qsTr("Snoozed"); icon: "clock"; count: root.model.snoozed; countIsUnread: false }
            UnifiedList { list: "done"; label: qsTr("Done"); icon: "done" }
            UnifiedList { list: "sent"; label: qsTr("Sent"); icon: "sent" }
            UnifiedList { list: "drafts"; label: qsTr("Drafts"); icon: "drafts"; count: root.model.drafts; countIsUnread: false }
            UnifiedList { list: "scheduled"; label: qsTr("Scheduled"); icon: "send"; count: root.model.scheduled || 0; countIsUnread: false }
            UnifiedList { list: "archive"; label: qsTr("Archive"); icon: "archive" }
            UnifiedList { list: "spam"; label: qsTr("Spam"); icon: "spam" }
            UnifiedList { list: "trash"; label: qsTr("Trash"); icon: "trash" }

            Item {
                width: col.width
                height: accHdr.height
                Header { id: accHdr; text: qsTr("Accounts") }
                IconButton {
                    anchors.right: parent.right
                    anchors.rightMargin: 4
                    anchors.bottom: parent.bottom
                    size: 24
                    icon: "plus"
                    tip: qsTr("Add account")
                    onClicked: root.openSettings("addAccount")
                }
            }
            Repeater {
                model: root.model.accounts
                delegate: Column {
                    id: acc
                    required property var modelData
                    width: col.width
                    spacing: 1
                    Row {
                        width: parent.width
                        Item {
                            width: 22; height: 32
                            visible: acc.modelData.folders.length > 0
                            IconImage {
                                anchors.centerIn: parent
                                width: 14; height: 14
                                sourceSize: Qt.size(28, 28)
                                source: "qrc:/qt/qml/org/rustyinbox/icons/chevron.svg"
                                color: Theme.textMuted
                                rotation: root.expanded[acc.modelData.email] ? 90 : 0
                                Behavior on rotation { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
                            }
                            TapHandler {
                                onTapped: {
                                    const e = Object.assign({}, root.expanded)
                                    e[acc.modelData.email] = !e[acc.modelData.email]
                                    root.expanded = e
                                }
                            }
                        }
                        NavItem {
                            width: parent.width - 22
                            target: ({ list: "inbox", account: acc.modelData.email })
                            label: root.accountLabel(acc.modelData)
                            icon: "account"
                            count: acc.modelData.unread
                        }
                    }
                    Repeater {
                        model: root.expanded[acc.modelData.email] ? acc.modelData.folders : []
                        delegate: NavItem {
                            required property var modelData
                            target: ({ list: "folder", folder: modelData.id })
                            label: modelData.path.replace(/^INBOX[./]/, "")
                            icon: "folder"
                            indent: 22
                        }
                    }
                }
            }
        }
    }
}
