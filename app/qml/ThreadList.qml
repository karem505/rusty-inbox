import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

// Conversation list. With `smart` set the rows are grouped (Pinned, People, Invitations,
// Notifications, Newsletters); automated groups start collapsed, like a smart inbox.
Rectangle {
    id: root
    color: Theme.list

    property var threads: []
    property bool smart: false
    property int openId: -1
    property var selected: ({})          // thread id -> true (multi-select)
    property int focusIndex: 0
    property var collapsed: ({ notifications: true, newsletters: true })
    readonly property var selectedIds: Object.keys(selected).map(Number)
    readonly property int rowCount: rowsModel.count
    property string title: ""
    // select mode: check boxes on every row, tapping a row selects it instead of opening it
    property bool selectMode: false
    readonly property bool selecting: selectMode || selectedIds.length > 0
    readonly property int visibleCount: { rowsModel.count; collapsed; let n = 0; for (let i = 0; i < rowsModel.count; i++) if (visibleRow(i)) n++; return n }
    readonly property bool allSelected: visibleCount > 0 && selectedIds.length >= visibleCount
    // list appearance from Settings
    property bool compact: false
    property bool showAvatars: true
    property bool showPreview: true
    // compact rows put the preview on the subject line (two lines instead of three)
    readonly property int rowHeight: compact ? 54 : (showPreview ? 76 : 58)

    signal open(int id)
    signal action(string name, var ids)
    signal snooze(var ids, Item anchor)

    readonly property var groupOrder: ["pinned", "personal", "invitations", "notifications", "newsletters"]
    readonly property var groupLabel: ({
        pinned: qsTr("Pinned"), personal: qsTr("People"), invitations: qsTr("Invitations"),
        notifications: qsTr("Notifications"), newsletters: qsTr("Newsletters"), all: ""
    })

    function groupOf(t) {
        if (!smart) return "all"
        if (t.pinned) return "pinned"
        return groupOrder.indexOf(t.category) >= 0 ? t.category : "personal"
    }

    // Never rebuild the model synchronously from a property change: that destroys delegates whose
    // bindings may still be evaluating (it crashed in QString::operator= inside a binding update).
    onThreadsChanged: Qt.callLater(rebuild)
    onSmartChanged: Qt.callLater(rebuild)

    // Reconcile the model with `threads` in place (remove, move, insert, set) so unchanged rows
    // keep their delegates: no flicker, scroll position kept, and far cheaper than clear + append.
    property bool logTiming: Qt.application.arguments.indexOf("--stress") >= 0
    function rebuild() {
        const t0 = Date.now()
        rebuildRows()
        if (logTiming) console.warn("REBUILD", rowsModel.count, Date.now() - t0, "ms")
    }
    function rebuildRows() {
        let rows = threads.map(t => Object.assign({ group: groupOf(t), tid: t.id, sender: t.from }, t))
        if (smart)
            rows.sort((a, b) => (groupOrder.indexOf(a.group) - groupOrder.indexOf(b.group)) || (b.date - a.date))
        const want = {}
        for (const r of rows) want[r.tid] = true
        for (let i = rowsModel.count - 1; i >= 0; i--)
            if (!want[rowsModel.get(i).tid]) rowsModel.remove(i)
        // JS mirror of the model's tid order, kept in step with every move/insert
        const order = []
        for (let i = 0; i < rowsModel.count; i++) order.push(rowsModel.get(i).tid)
        for (let i = 0; i < rows.length; i++) {
            const r = rows[i]
            if (order[i] !== r.tid) {
                const j = order.indexOf(r.tid, i + 1)
                if (j > i) {
                    rowsModel.move(j, i, 1)
                    order.splice(i, 0, order.splice(j, 1)[0])
                } else {
                    rowsModel.insert(i, r)
                    order.splice(i, 0, r.tid)
                }
            }
            const now = rowsModel.get(i)
            let changed = false
            for (const key in r) if (now[key] !== r[key]) { changed = true; break }
            if (changed) rowsModel.set(i, r)
        }
        const keep = {}
        for (const r of rows) if (selected[r.tid]) keep[r.tid] = true
        selected = keep
        focusIndex = Math.min(Math.max(0, focusIndex), Math.max(0, rowsModel.count - 1))
        if (!visibleRow(focusIndex)) moveFocus(1)
    }

    function idAt(i) { return i >= 0 && i < rowsModel.count ? rowsModel.get(i).tid : -1 }
    function indexOf(id) {
        for (let i = 0; i < rowsModel.count; i++) if (rowsModel.get(i).tid === id) return i
        return -1
    }
    function visibleRow(i) { return i >= 0 && i < rowsModel.count && !collapsed[rowsModel.get(i).group] }
    function focusedId() { return idAt(focusIndex) }

    // Keyboard focus moves over visible rows only.
    function moveFocus(delta) {
        let i = focusIndex
        for (let n = 0; n < rowsModel.count; n++) {
            i += delta
            if (i < 0 || i >= rowsModel.count) return false
            if (visibleRow(i)) { focusIndex = i; list.positionViewAtIndex(i, ListView.Contain); return true }
        }
        return false
    }
    // The visible row after (or else before) `id`, to open once `id` leaves the list.
    function neighbourOf(id) {
        const i = indexOf(id)
        for (let j = i + 1; j < rowsModel.count; j++) if (visibleRow(j) && !selected[idAt(j)]) return idAt(j)
        for (let j = i - 1; j >= 0; j--) if (visibleRow(j) && !selected[idAt(j)]) return idAt(j)
        return -1
    }
    // Ids an action applies to: the multi-selection, else the opened thread, else the focused row.
    function targetIds() {
        if (selectedIds.length) return selectedIds
        if (openId >= 0) return [openId]
        const id = idAt(focusIndex)
        return id >= 0 ? [id] : []
    }
    function rowOf(id) { const i = indexOf(id); return i >= 0 ? rowsModel.get(i) : null }
    function clearSelection() { selected = ({}); selectMode = false }
    function toggle(id) {
        const s = Object.assign({}, selected)
        if (s[id]) delete s[id]; else s[id] = true
        selected = s
    }
    function toggleFocused() { const id = idAt(focusIndex); if (id >= 0) { selectMode = true; toggle(id) } }
    function groupState(group) {
        let n = 0, sel = 0
        for (let i = 0; i < rowsModel.count; i++) { const r = rowsModel.get(i); if (r.group === group) { n++; if (selected[r.tid]) sel++ } }
        return n === 0 ? 0 : (sel === n ? 2 : (sel > 0 ? 1 : 0))
    }
    function selectGroup(group, on) {
        const s = Object.assign({}, selected)
        for (let i = 0; i < rowsModel.count; i++) { const r = rowsModel.get(i); if (r.group === group) { if (on) s[r.tid] = true; else delete s[r.tid] } }
        selected = s
        selectMode = true
    }
    function selectAll() {
        const s = {}
        for (let i = 0; i < rowsModel.count; i++) if (visibleRow(i)) s[idAt(i)] = true
        selected = s
        selectMode = true
    }
    function unreadIn(group) {
        let n = 0
        for (let i = 0; i < rowsModel.count; i++) { const r = rowsModel.get(i); if (r.group === group && r.unread) n++ }
        return n
    }

    function when(ms) {
        const d = new Date(ms)
        const now = new Date()
        if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString(Qt.locale(), "hh:mm")
        if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString(Qt.locale(), "d MMM")
        return d.toLocaleDateString(Qt.locale(), "d MMM yyyy")
    }
    function htmlEsc(t) { return String(t).replace(/\s+/g, " ").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;") }
    function initials(name) {
        const parts = (name || "?").replace(/[^\p{L}\p{N} ]/gu, "").trim().split(/\s+/)
        return ((parts[0] || "?")[0] + (parts.length > 1 ? parts[parts.length - 1][0] : "")).toUpperCase()
    }

    ListModel { id: rowsModel }

    // list toolbar: title + Select / Select all; turns into the bulk-action bar while selecting
    Rectangle {
        id: bulk
        anchors.top: parent.top
        width: parent.width
        height: 44
        color: root.selecting ? Theme.surface : Theme.list
        Behavior on color { ColorAnimation { duration: Theme.fast } }
        z: 2
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }

        // idle: title and the two selection buttons
        Row {
            visible: !root.selecting
            anchors.verticalCenter: parent.verticalCenter
            x: 16
            spacing: 8
            Text {
                text: root.title
                font.pixelSize: 15
                font.weight: Font.DemiBold
                color: Theme.textStrong
                anchors.verticalCenter: parent.verticalCenter
            }
            Text {
                text: root.visibleCount
                visible: root.visibleCount > 0
                font.pixelSize: 12
                color: Theme.textMuted
                anchors.verticalCenter: parent.verticalCenter
            }
        }
        Row {
            visible: !root.selecting
            anchors.right: parent.right
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            IconButton { objectName: "selectButton"; icon: "select"; tip: qsTr("Select (X)"); onClicked: root.selectMode = true }
            IconButton { objectName: "selectAllButton"; icon: "checkbox"; tip: qsTr("Select all (Ctrl+A)"); onClicked: root.selectAll() }
        }

        // selecting: master check box, count, select all, actions, cancel
        Row {
            visible: root.selecting
            anchors.verticalCenter: parent.verticalCenter
            x: 14
            spacing: 6
            TickBox {
                anchors.verticalCenter: parent.verticalCenter
                checked: root.allSelected
                partial: root.selectedIds.length > 0
                onToggled: root.allSelected ? root.selected = ({}) : root.selectAll()
            }
            Text {
                text: root.selectedIds.length ? qsTr("%1 selected").arg(root.selectedIds.length) : qsTr("Select conversations")
                font.pixelSize: 13
                color: Theme.text
                anchors.verticalCenter: parent.verticalCenter
                leftPadding: 4
                rightPadding: 4
            }
            Text {
                visible: !root.allSelected
                text: qsTr("Select all")
                color: Theme.accent
                font.pixelSize: 13
                anchors.verticalCenter: parent.verticalCenter
                rightPadding: 8
                HoverHandler { cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: root.selectAll() }
            }
            Row {
                spacing: 2
                anchors.verticalCenter: parent.verticalCenter
                enabled: root.selectedIds.length > 0
                opacity: enabled ? 1 : 0.4
                IconButton { icon: "done"; tip: qsTr("Done (E)"); onClicked: root.action("done", root.selectedIds) }
                IconButton { icon: "pin"; tip: qsTr("Pin (P)"); onClicked: root.action("pin", root.selectedIds) }
                IconButton { id: bulkSnooze; icon: "clock"; tip: qsTr("Snooze (S)"); onClicked: root.snooze(root.selectedIds, bulkSnooze) }
                IconButton { icon: "unread"; tip: qsTr("Mark as unread (U)"); onClicked: root.action("markAsUnseen", root.selectedIds) }
                IconButton { icon: "archive"; tip: qsTr("Archive"); onClicked: root.action("archive", root.selectedIds) }
                IconButton { icon: "spam"; tip: qsTr("Spam (!)"); onClicked: root.action("markAsSpam", root.selectedIds) }
                IconButton { icon: "trash"; tip: qsTr("Delete (#)"); onClicked: root.action("moveToTrash", root.selectedIds) }
            }
        }
        IconButton {
            visible: root.selecting
            anchors.right: parent.right
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            icon: "close"
            tip: qsTr("Cancel (Esc)")
            onClicked: root.clearSelection()
        }
    }

    ListView {
        id: list
        anchors.top: bulk.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        clip: true
        model: rowsModel
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}

        // rows slide in / fade out; the rest glide into place (not on first fill: `populate` stays unset)
        add: Transition {
            NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.normal; easing.type: Theme.easing }
            NumberAnimation { property: "x"; from: 24; to: 0; duration: Theme.normal; easing.type: Theme.easing }
        }
        remove: Transition {
            NumberAnimation { property: "opacity"; to: 0; duration: Theme.fast }
            NumberAnimation { property: "x"; to: -40; duration: Theme.normal; easing.type: Theme.easing }
        }
        displaced: Transition {
            NumberAnimation { properties: "x,y"; duration: Theme.normal; easing.type: Theme.easing }
            NumberAnimation { property: "opacity"; to: 1; duration: Theme.fast }
        }
        move: Transition {
            NumberAnimation { properties: "x,y"; duration: Theme.normal; easing.type: Theme.easing }
        }

        section.property: root.smart ? "group" : ""
        section.delegate: Rectangle {
            id: sec
            required property string section
            width: ListView.view.width
            height: 34
            color: Theme.section
            readonly property int unread: { rowsModel.count; return root.unreadIn(section) }
            Row {
                x: 14
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                TickBox {
                    visible: root.selecting
                    boxSize: 15
                    anchors.verticalCenter: parent.verticalCenter
                    readonly property int st: { root.selected; rowsModel.count; return root.groupState(sec.section) }
                    checked: st === 2
                    partial: st === 1
                    onToggled: root.selectGroup(sec.section, st !== 2)
                }
                IconImage {
                    width: 12; height: 12
                    anchors.verticalCenter: parent.verticalCenter
                    sourceSize: Qt.size(24, 24)
                    source: "qrc:/qt/qml/org/rustyinbox/icons/chevron.svg"
                    color: Theme.textSubtle
                    rotation: root.collapsed[sec.section] ? 0 : 90
                    Behavior on rotation { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
                }
                Text {
                    text: root.groupLabel[sec.section] || sec.section
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                    font.capitalization: Font.AllUppercase
                    color: Theme.textSubtle
                }
                Text {
                    visible: sec.unread > 0
                    text: qsTr("%1 new").arg(sec.unread)
                    font.pixelSize: 12
                    color: Theme.accent
                }
            }
            HoverHandler { cursorShape: Qt.PointingHandCursor }
            TapHandler {
                onTapped: {
                    const c = Object.assign({}, root.collapsed)
                    c[sec.section] = !c[sec.section]
                    root.collapsed = c
                }
            }
        }

        delegate: Rectangle {
            id: row
            required property int index
            required property int tid
            required property string group
            required property string sender
            required property string fromAddr
            required property string subject
            required property string preview
            required property double date
            required property int count
            required property bool unread
            required property bool pinned

            readonly property bool isCollapsed: root.collapsed[group] === true
            readonly property bool isSelected: root.selected[tid] === true
            width: ListView.view.width
            height: isCollapsed ? 0 : root.rowHeight
            visible: height > 0
            clip: true
            Behavior on height { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
            color: isSelected || tid === root.openId ? Theme.selected : (hover.hovered ? Theme.hover : Theme.list)
            Behavior on color { ColorAnimation { duration: Theme.fast } }

            HoverHandler { id: hover }
            TapHandler {
                acceptedModifiers: Qt.NoModifier
                onTapped: {
                    root.focusIndex = row.index
                    if (root.selecting) root.toggle(row.tid)
                    else root.open(row.tid)
                }
            }
            TapHandler {
                acceptedModifiers: Qt.ControlModifier
                onTapped: {
                    const s = Object.assign({}, root.selected)
                    if (s[row.tid]) delete s[row.tid]; else s[row.tid] = true
                    root.selected = s
                    root.focusIndex = row.index
                }
            }
            TapHandler {
                acceptedModifiers: Qt.ShiftModifier
                onTapped: {
                    const a = Math.min(root.focusIndex, row.index), b = Math.max(root.focusIndex, row.index)
                    const s = Object.assign({}, root.selected)
                    for (let i = a; i <= b; i++) if (root.visibleRow(i)) s[root.idAt(i)] = true
                    root.selected = s
                }
            }

            // keyboard focus marker
            Rectangle {
                visible: row.index === root.focusIndex
                width: 3; height: parent.height
                color: Theme.accent
            }

            // check box in place of the avatar while selecting (or when hovering the avatar)
            Item {
                id: pick
                x: 14
                anchors.verticalCenter: parent.verticalCenter
                width: root.showAvatars ? (root.compact ? 30 : 36) : 20
                height: width
                readonly property bool showBox: root.selecting || row.isSelected || avatarHover.hovered || !root.showAvatars && hover.hovered
                HoverHandler { id: avatarHover }
                TickBox {
                    anchors.centerIn: parent
                    visible: pick.showBox
                    checked: row.isSelected
                    onToggled: { root.selectMode = true; root.toggle(row.tid); root.focusIndex = row.index }
                }
            }
            Rectangle {
                id: avatar
                visible: root.showAvatars && !pick.showBox
                anchors.centerIn: pick
                width: pick.width; height: width; radius: width / 2
                color: Theme.avatarFor(row.fromAddr)
                Text {
                    anchors.centerIn: parent
                    text: root.initials(row.sender)
                    color: Theme.dark ? Theme.window : "white"
                    font.pixelSize: 13
                    font.weight: Font.DemiBold
                }
                Rectangle {
                    visible: row.unread
                    width: 10; height: 10; radius: 5
                    color: Theme.accent
                    border.color: Theme.list; border.width: 1.5
                    scale: row.unread ? 1 : 0
                    Behavior on scale { NumberAnimation { duration: Theme.normal; easing.type: Easing.OutBack } }
                    anchors.right: parent.right; anchors.top: parent.top
                }
            }

            Column {
                anchors.left: pick.right
                anchors.leftMargin: 12
                anchors.right: parent.right
                anchors.rightMargin: 14
                anchors.verticalCenter: parent.verticalCenter
                spacing: 3

                Item {
                    width: parent.width
                    height: senderText.height
                    Text {
                        id: senderText
                        width: parent.width - meta.width - 8
                        text: row.sender + (row.count > 1 ? "  " + row.count : "")
                        elide: Text.ElideRight
                        font.pixelSize: 14
                        font.weight: row.unread ? Font.DemiBold : Font.Normal
                        color: Theme.textStrong
                    }
                    Row {
                        id: meta
                        anchors.right: parent.right
                        spacing: 6
                        opacity: hover.hovered ? 0 : 1
                        Behavior on opacity { NumberAnimation { duration: Theme.fast } }
                        IconImage {
                            visible: row.pinned
                            width: 13; height: 13
                            sourceSize: Qt.size(26, 26)
                            source: "qrc:/qt/qml/org/rustyinbox/icons/pin.svg"
                            color: Theme.accent
                            anchors.verticalCenter: parent.verticalCenter
                        }
                        Text {
                            text: root.when(row.date)
                            font.pixelSize: 12
                            color: row.unread ? Theme.accent : Theme.textMuted
                        }
                    }
                }
                Text {
                    width: parent.width
                    textFormat: Text.StyledText
                    text: root.compact && root.showPreview && row.preview
                        ? root.htmlEsc(row.subject || qsTr("(no subject)")) + "<font color='" + Theme.textMuted + "'>  " + root.htmlEsc(row.preview) + "</font>"
                        : root.htmlEsc(row.subject || qsTr("(no subject)"))
                    elide: Text.ElideRight
                    font.pixelSize: 13
                    font.weight: row.unread ? Font.DemiBold : Font.Normal
                    color: Theme.text
                }
                Text {
                    visible: root.showPreview && !root.compact
                    width: parent.width
                    text: row.preview
                    elide: Text.ElideRight
                    font.pixelSize: 12
                    color: Theme.textMuted
                }
            }

            // quick actions on hover
            Rectangle {
                readonly property bool show: hover.hovered && !root.selecting
                visible: opacity > 0
                opacity: show ? 1 : 0
                anchors.right: parent.right
                anchors.rightMargin: show ? 10 : 2
                Behavior on opacity { NumberAnimation { duration: Theme.fast } }
                Behavior on anchors.rightMargin { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
                y: 6
                width: quick.implicitWidth + 6
                height: 30
                radius: 8
                color: Theme.list
                border.color: Theme.border
                Row {
                    id: quick
                    anchors.centerIn: parent
                    spacing: 1
                    IconButton { size: 26; icon: "done"; tip: qsTr("Done"); onClicked: root.action("done", [row.tid]) }
                    IconButton { size: 26; icon: "pin"; active: row.pinned; tip: row.pinned ? qsTr("Unpin") : qsTr("Pin"); onClicked: root.action(row.pinned ? "unpin" : "pin", [row.tid]) }
                    IconButton { id: rowSnooze; size: 26; icon: "clock"; tip: qsTr("Snooze"); onClicked: root.snooze([row.tid], rowSnooze) }
                    IconButton { size: 26; icon: "trash"; tip: qsTr("Delete"); onClicked: root.action("moveToTrash", [row.tid]) }
                }
            }

            Rectangle {
                anchors.bottom: parent.bottom
                x: 62
                width: parent.width - 62
                height: 1
                color: Theme.border
            }
        }

        Text {
            anchors.centerIn: parent
            visible: list.count === 0
            text: qsTr("Nothing here")
            color: Theme.textMuted
            font.pixelSize: 15
        }
    }
}
