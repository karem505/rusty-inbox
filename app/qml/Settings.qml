import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

// Settings window (full-window page over the mail view). Every option here is wired to the app:
// values are saved through the backend (`rustyInbox.*` settings) and applied live.
Rectangle {
    id: root
    property var backend
    property string page: "general"
    property var prefs: ({})
    property var accounts: []
    property string confirmRemove: ""     // email awaiting removal confirmation
    property string busyEmail: ""         // add-account request in flight
    property string addError: ""
    property bool showManual: false
    signal closed()
    signal notice(string text)
    property alias aiPage: aiSettingsPage     // scripted check (Main.qml, AI(B))
    function scrollTo(y) { scroller.contentItem.contentY = y }
    function scrollToItem(it, above) {
        const p = it.mapToItem(scroller.contentItem.contentItem, 0, 0)
        scroller.contentItem.contentY = Math.max(0, p.y - 20 - (above || 0))
    }
    function pageHeight() { return scroller.contentItem.contentHeight - scroller.height }

    color: Theme.window
    focus: true

    // the page sits over the mail view: swallow every click, hover and scroll it doesn't use,
    // so nothing reaches the sidebar or list underneath
    MouseArea {
        anchors.fill: parent
        z: -1
        hoverEnabled: true
        acceptedButtons: Qt.AllButtons
        onWheel: (wheel) => wheel.accepted = true
    }
    Keys.onEscapePressed: (root.page === "addAccount" || root.page === "importTb") ? root.page = "accounts" : root.closed()

    property var templates: []
    // Thunderbird import
    property var tbAccounts: []
    property var tbPicked: ({})
    property var tbStatus: ({})          // email -> "busy" | "ok" | error text
    property var tbPasswords: ({})
    function openImport() {
        tbAccounts = JSON.parse(backend.thunderbird_json())
        const picked = {}
        for (const a of tbAccounts) picked[a.email] = !a.exists
        tbPicked = picked
        tbStatus = ({})
        page = "importTb"
    }
    function runImport() {
        const emails = tbAccounts.filter(a => tbPicked[a.email] && !a.exists).map(a => a.email)
        if (!emails.length) return
        const st = Object.assign({}, tbStatus)
        for (const e of emails) st[e] = "busy"
        tbStatus = st
        backend.import_thunderbird(JSON.stringify({ emails: emails, passwords: tbPasswords }))
    }
    // update state (see updater.rs): refreshed from the backend and by update_changed
    property var upd: ({})
    readonly property bool updBusy: upd.state === "checking" || upd.state === "downloading"
    function reloadUpdate() { upd = JSON.parse(backend.update_json()) }
    function fmtDate(iso) { return iso ? new Date(iso).toLocaleDateString(Qt.locale(), Locale.LongFormat) : "" }
    function fmtMb(b) { return (b / 1048576).toFixed(1) }
    // Qt draws inline `code` in a larger fixed font than the text around it: keep it plain
    function plainCode(md) { return (md || "").replace(/`([^`\n]+)`/g, "$1") }
    onPageChanged: if (page === "updates") { reloadUpdate(); if (upd.state === "idle") backend.check_update() }
    function reload() {
        reloadUpdate()
        prefs = JSON.parse(backend.settings_json())
        accounts = JSON.parse(backend.account_details_json())
        templates = JSON.parse(backend.templates_json())
    }
    function save(key, value) {
        const err = backend.set_setting(key, JSON.stringify(value))
        if (err !== "") root.notice(err)
        reload()
    }
    Component.onCompleted: reload()
    onVisibleChanged: if (visible) reload()

    Connections {
        target: root.backend
        function onImport_result(email, error) {
            const st = Object.assign({}, root.tbStatus)
            st[email] = error === "" ? "ok" : error
            root.tbStatus = st
            if (error === "") {
                root.notice(qsTr("%1 added. Its mail is downloading now.").arg(email))
                root.reload()
                root.tbAccounts = root.tbAccounts.map(a => a.email === email ? Object.assign({}, a, { exists: true }) : a)
            }
        }
        function onUpdate_changed(json) { root.upd = JSON.parse(json) }
        function onAccount_added(email, error) {
            root.busyEmail = ""
            if (error === "") {
                root.addError = ""
                root.reload()
                root.page = "accounts"
                root.notice(qsTr("%1 added. Its mail is downloading now.").arg(email))
                form.clear()
            } else {
                root.addError = error
            }
        }
    }

    readonly property var pages: [
        { id: "general", label: qsTr("General"), icon: "settings" },
        { id: "accounts", label: qsTr("Accounts"), icon: "account" },
        { id: "notifications", label: qsTr("Notifications"), icon: "bell" },
        { id: "appearance", label: qsTr("Appearance"), icon: "palette" },
        { id: "ai", label: qsTr("AI"), icon: "sparkle" },
        { id: "templates", label: qsTr("Templates"), icon: "template" },
        { id: "shortcuts", label: qsTr("Shortcuts"), icon: "keyboard" },
        { id: "updates", label: qsTr("Updates"), icon: "update" },
        { id: "about", label: qsTr("About"), icon: "info" }
    ]

    // ---- building blocks -------------------------------------------------------------------

    component Title: Text {
        font.pixelSize: 22
        font.weight: Font.DemiBold
        color: Theme.textStrong
        bottomPadding: 8
    }
    component Group: Text {
        font.pixelSize: 11
        font.weight: Font.DemiBold
        font.capitalization: Font.AllUppercase
        color: Theme.textMuted
        topPadding: 18
        bottomPadding: 6
    }
    // one option: label + help text on the left, the control on the right
    component Option: Rectangle {
        id: opt
        property string label
        property string help
        default property alias control: slot.data
        Layout.fillWidth: true
        implicitHeight: Math.max(56, textCol.implicitHeight + 22)
        radius: 10
        color: Theme.surface
        Column {
            id: textCol
            anchors.left: parent.left
            anchors.leftMargin: 16
            anchors.right: slot.left
            anchors.rightMargin: 16
            anchors.verticalCenter: parent.verticalCenter
            spacing: 3
            Text { width: parent.width; text: opt.label; font.pixelSize: 14; color: Theme.text; wrapMode: Text.Wrap }
            Text {
                width: parent.width
                visible: opt.help !== ""
                text: opt.help
                font.pixelSize: 12
                color: Theme.textMuted
                wrapMode: Text.Wrap
            }
        }
        Item {
            id: slot
            anchors.right: parent.right
            anchors.rightMargin: 14
            anchors.verticalCenter: parent.verticalCenter
            width: childrenRect.width
            height: childrenRect.height
        }
    }
    // on/off switch in the theme colours
    component Toggle: Rectangle {
        id: tg
        property string key
        readonly property bool on: root.prefs[key] === true
        width: 42
        height: 24
        radius: 12
        color: on ? Theme.accent : Theme.border
        Behavior on color { ColorAnimation { duration: Theme.fast } }
        opacity: enabled ? 1 : 0.5
        Rectangle {
            width: 18; height: 18; radius: 9
            y: 3
            x: tg.on ? tg.width - width - 3 : 3
            color: tg.on ? Theme.accentText : Theme.window
            Behavior on x { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
        }
        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: if (tg.enabled) root.save(tg.key, !tg.on) }
    }
    // segmented choice for a fixed set of values
    component Choice: Row {
        id: ch
        property string key
        property var values: []
        property var labels: []
        spacing: 0
        Repeater {
            model: ch.values.length
            delegate: Rectangle {
                required property int index
                readonly property bool on: root.prefs[ch.key] === ch.values[index]
                width: lab.implicitWidth + 22
                height: 30
                radius: 7
                color: on ? Theme.accent : (h.hovered ? Theme.hover : "transparent")
                border.color: on ? Theme.accent : Theme.border
                Behavior on color { ColorAnimation { duration: Theme.fast } }
                Text {
                    id: lab
                    anchors.centerIn: parent
                    text: ch.labels[index]
                    font.pixelSize: 13
                    color: parent.on ? Theme.accentText : Theme.text
                }
                HoverHandler { id: h; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: root.save(ch.key, ch.values[index]) }
            }
        }
    }
    component Button2: Rectangle {
        id: btn
        property string text
        property bool primary: false
        property bool danger: false
        signal clicked()
        implicitWidth: t.implicitWidth + 28
        implicitHeight: 34
        radius: 8
        opacity: enabled ? 1 : 0.5
        color: primary ? (h2.hovered ? Qt.darker(Theme.accent, 1.08) : Theme.accent)
                       : danger ? (h2.hovered ? Qt.rgba(1, 0.3, 0.3, 0.18) : "transparent")
                                : (h2.hovered ? Theme.hover : "transparent")
        border.color: primary ? Theme.accent : (danger ? Theme.danger : Theme.border)
        Behavior on color { ColorAnimation { duration: Theme.fast } }
        Text {
            id: t
            anchors.centerIn: parent
            text: btn.text
            font.pixelSize: 13
            font.weight: Font.DemiBold
            color: btn.primary ? Theme.accentText : (btn.danger ? Theme.danger : Theme.text)
        }
        HoverHandler { id: h2; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: if (btn.enabled) btn.clicked() }
    }
    component Field: Column {
        id: fld
        property string label
        property alias text: input.text
        property alias placeholder: input.placeholderText
        property bool secret: false
        property alias input: input
        Layout.fillWidth: true
        spacing: 5
        Text { text: fld.label; font.pixelSize: 12; color: Theme.textSubtle }
        TextField {
            id: input
            width: parent.width
            echoMode: fld.secret ? TextInput.Password : TextInput.Normal
            color: Theme.text
            font.pixelSize: 14
            selectByMouse: true
            background: Rectangle {
                radius: 8
                color: Theme.surface
                border.color: input.activeFocus ? Theme.accent : Theme.border
                border.width: input.activeFocus ? 1.5 : 1
            }
        }
    }

    // ---- layout ----------------------------------------------------------------------------

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // page list
        Rectangle {
            Layout.preferredWidth: 230
            Layout.fillHeight: true
            color: Theme.sidebar
            Column {
                anchors.fill: parent
                anchors.margins: 12
                spacing: 2
                Row {
                    spacing: 8
                    bottomPadding: 14
                    IconButton { icon: "back"; tip: qsTr("Back to mail (Esc)"); onClicked: root.closed() }
                    Text {
                        text: qsTr("Settings")
                        font.pixelSize: 18
                        font.weight: Font.DemiBold
                        color: Theme.textStrong
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }
                Repeater {
                    model: root.pages
                    delegate: Rectangle {
                        required property var modelData
                        readonly property bool current: root.page === modelData.id || (modelData.id === "accounts" && (root.page === "addAccount" || root.page === "importTb"))
                        width: parent.width
                        height: 34
                        radius: 8
                        color: current ? Theme.navSelected : (ph.hovered ? Theme.hover : "transparent")
                        Behavior on color { ColorAnimation { duration: Theme.fast } }
                        IconImage {
                            id: pic
                            x: 10
                            anchors.verticalCenter: parent.verticalCenter
                            width: 18; height: 18
                            sourceSize: Qt.size(36, 36)
                            source: "qrc:/qt/qml/org/rustyinbox/icons/" + modelData.icon + ".svg"
                            color: parent.current ? Theme.accent : Theme.icon
                        }
                        Text {
                            anchors.left: pic.right
                            anchors.leftMargin: 10
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label
                            font.pixelSize: 14
                            font.weight: parent.current ? Font.DemiBold : Font.Normal
                            color: Theme.text
                        }
                        HoverHandler { id: ph; cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: { root.page = modelData.id; root.confirmRemove = "" } }
                    }
                }
            }
        }
        Rectangle { Layout.preferredWidth: 1; Layout.fillHeight: true; color: Theme.border }

        // page content
        ScrollView {
            id: scroller
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            clip: true

            Item {
                width: scroller.availableWidth
                implicitHeight: content.implicitHeight + 60

                ColumnLayout {
                    id: content
                    x: Math.max(28, (parent.width - 720) / 2)
                    y: 28
                    width: Math.min(720, parent.width - 56)
                    spacing: 8

                    // ---------------- General
                    ColumnLayout {
                        visible: root.page === "general"
                        Layout.fillWidth: true
                        spacing: 8
                        Title { text: qsTr("General") }
                        Group { text: qsTr("Inbox") }
                        Option {
                            label: qsTr("Smart Inbox")
                            help: qsTr("Group the inbox into People, Pinned, Invitations, Notifications and Newsletters.")
                            Toggle { key: "smartInbox" }
                        }
                        Option {
                            label: qsTr("Collapse automated mail")
                            help: qsTr("Notifications and Newsletters start folded so people come first.")
                            Toggle { key: "collapseAutomated" }
                        }
                        Option {
                            label: qsTr("Show Done mail in the Inbox")
                            help: qsTr("Done conversations stay in the Inbox, dimmed and marked with a check. Off: they move to Done. Also the eye button above the list.")
                            Toggle { key: "showDoneInInbox" }
                        }
                        Option {
                            label: qsTr("Mark as read when opened")
                            help: qsTr("Opening a conversation marks it read. Off: use U to mark it read yourself.")
                            Toggle { key: "markReadOnOpen" }
                        }
                        Option {
                            label: qsTr("Undo time")
                            help: qsTr("How long Done, Delete, Snooze and Spam can be undone.")
                            Choice { key: "undoSeconds"; values: [5, 10, 20, 30]; labels: ["5 s", "10 s", "20 s", "30 s"] }
                        }
                        Group { text: qsTr("Sync") }
                        Option {
                            label: qsTr("Check for new mail")
                            help: qsTr("How often every account is checked.")
                            Choice {
                                key: "syncMinutes"
                                values: [1, 2, 5, 15, 30]
                                labels: [qsTr("1 min"), qsTr("2 min"), qsTr("5 min"), qsTr("15 min"), qsTr("30 min")]
                            }
                        }
                        Option {
                            label: qsTr("Keep running in the status bar")
                            help: qsTr("Closing the window keeps %1 in the top bar so mail and notifications keep arriving. Quit from the bar icon.").arg(root.backend.app_name())
                            Toggle { key: "closeToTray" }
                        }
                    }

                    // ---------------- Accounts
                    ColumnLayout {
                        visible: root.page === "accounts"
                        Layout.fillWidth: true
                        spacing: 8
                        RowLayout {
                            Layout.fillWidth: true
                            Title { text: qsTr("Accounts"); Layout.fillWidth: true }
                            Button2 { objectName: "importTbButton"; text: qsTr("Import from Thunderbird"); onClicked: root.openImport() }
                            Button2 { objectName: "addAccountButton"; text: qsTr("Add account"); primary: true; onClicked: root.page = "addAccount" }
                        }
                        Text {
                            visible: root.accounts.length === 0
                            text: qsTr("No accounts yet. Add one to start.")
                            color: Theme.textMuted
                            font.pixelSize: 14
                        }
                        Repeater {
                            model: root.accounts
                            delegate: Rectangle {
                                id: card
                                required property var modelData
                                property bool editing: false
                                Layout.fillWidth: true
                                implicitHeight: cardCol.implicitHeight + 28
                                radius: 12
                                color: Theme.surface
                                ColumnLayout {
                                    id: cardCol
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    anchors.margins: 14
                                    spacing: 8
                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: 12
                                        Rectangle {
                                            width: 38; height: 38; radius: 19
                                            color: Theme.avatarFor(card.modelData.email)
                                            Text {
                                                anchors.centerIn: parent
                                                text: (card.modelData.name || card.modelData.email).charAt(0).toUpperCase()
                                                color: Theme.dark ? Theme.window : "white"
                                                font.pixelSize: 15
                                                font.weight: Font.DemiBold
                                            }
                                        }
                                        Column {
                                            Layout.fillWidth: true
                                            spacing: 2
                                            Text {
                                                visible: !card.editing
                                                text: card.modelData.name || card.modelData.email
                                                font.pixelSize: 15
                                                font.weight: Font.DemiBold
                                                color: Theme.textStrong
                                            }
                                            TextField {
                                                id: nameEdit
                                                visible: card.editing
                                                width: 260
                                                text: card.modelData.name
                                                placeholderText: qsTr("Display name")
                                                color: Theme.text
                                                onAccepted: saveName.clicked()
                                            }
                                            Text { text: card.modelData.email; font.pixelSize: 12; color: Theme.textMuted }
                                        }
                                        Button2 {
                                            id: saveName
                                            text: card.editing ? qsTr("Save") : qsTr("Rename")
                                            primary: card.editing
                                            onClicked: {
                                                if (card.editing) {
                                                    const err = root.backend.rename_account(card.modelData.email, nameEdit.text)
                                                    if (err !== "") root.notice(err)
                                                    root.reload()
                                                }
                                                card.editing = !card.editing
                                            }
                                        }
                                        Button2 {
                                            text: qsTr("Remove")
                                            danger: true
                                            onClicked: root.confirmRemove = card.modelData.email
                                        }
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: qsTr("Incoming %1:%2 (%3)   ·   Outgoing %4:%5 (%6)")
                                            .arg(card.modelData.imapHost).arg(card.modelData.imapPort).arg(card.modelData.imapSecurity.toUpperCase())
                                            .arg(card.modelData.smtpHost).arg(card.modelData.smtpPort).arg(card.modelData.smtpSecurity.toUpperCase())
                                        font.pixelSize: 12
                                        color: Theme.textSubtle
                                        wrapMode: Text.Wrap
                                    }
                                    // signature
                                    Column {
                                        Layout.fillWidth: true
                                        spacing: 6
                                        Text { text: qsTr("Signature"); font.pixelSize: 12; color: Theme.textSubtle }
                                        Rectangle {
                                            width: parent.width
                                            height: Math.max(64, sigEdit.implicitHeight + 16)
                                            radius: 8
                                            color: Theme.window
                                            border.color: sigEdit.activeFocus ? Theme.accent : Theme.border
                                            TextEdit {
                                                id: sigEdit
                                                anchors.fill: parent
                                                anchors.margins: 8
                                                text: root.backend.signature(card.modelData.email)
                                                wrapMode: TextEdit.Wrap
                                                font.pixelSize: 13
                                                color: Theme.text
                                                selectByMouse: true
                                                Text {
                                                    visible: !sigEdit.text && !sigEdit.activeFocus
                                                    text: qsTr("Added to new messages, replies and forwards")
                                                    font.pixelSize: 13
                                                    color: Theme.textMuted
                                                }
                                                onActiveFocusChanged: if (!activeFocus) {
                                                    const err = root.backend.set_signature(card.modelData.email, text)
                                                    if (err !== "") root.notice(err)
                                                }
                                            }
                                        }
                                    }
                                    // removal confirmation
                                    Rectangle {
                                        visible: root.confirmRemove === card.modelData.email
                                        Layout.fillWidth: true
                                        implicitHeight: confirmRow.implicitHeight + 20
                                        radius: 8
                                        color: Qt.rgba(1, 0.3, 0.3, 0.10)
                                        border.color: Theme.danger
                                        RowLayout {
                                            id: confirmRow
                                            anchors.fill: parent
                                            anchors.margins: 10
                                            spacing: 10
                                            Text {
                                                Layout.fillWidth: true
                                                text: qsTr("Remove %1? Its mail is removed from this computer only; nothing is deleted on the server.").arg(card.modelData.email)
                                                wrapMode: Text.Wrap
                                                font.pixelSize: 13
                                                color: Theme.text
                                            }
                                            Button2 { text: qsTr("Cancel"); onClicked: root.confirmRemove = "" }
                                            Button2 {
                                                text: qsTr("Remove account")
                                                danger: true
                                                onClicked: {
                                                    const email = card.modelData.email
                                                    const err = root.backend.remove_account(email)
                                                    root.confirmRemove = ""
                                                    root.reload()
                                                    root.notice(err === "" ? qsTr("%1 removed").arg(email) : err)
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ---------------- Import from Thunderbird
                    ColumnLayout {
                        visible: root.page === "importTb"
                        Layout.fillWidth: true
                        spacing: 10
                        Title { text: qsTr("Import from Thunderbird") }
                        Text {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            text: root.tbAccounts.length
                                ? qsTr("Accounts found in your Thunderbird profile. Servers and saved passwords come over as they are; each account is checked with its server before it is added.")
                                : qsTr("No Thunderbird profile with mail accounts was found on this computer.")
                            font.pixelSize: 13
                            color: Theme.textSubtle
                        }
                        Repeater {
                            model: root.tbAccounts
                            delegate: Rectangle {
                                id: trow
                                required property var modelData
                                readonly property string status: root.tbStatus[modelData.email] || ""
                                readonly property bool needsPassword: !modelData.hasPassword && !modelData.exists
                                Layout.fillWidth: true
                                implicitHeight: trowCol.implicitHeight + 24
                                radius: 10
                                color: Theme.surface
                                border.color: Theme.border
                                TickBox {
                                    id: ttick
                                    x: 14
                                    y: 16
                                    visible: !trow.modelData.exists && trow.status !== "ok"
                                    checked: root.tbPicked[trow.modelData.email] === true
                                    onToggled: {
                                        const p = Object.assign({}, root.tbPicked)
                                        p[trow.modelData.email] = !p[trow.modelData.email]
                                        root.tbPicked = p
                                    }
                                }
                                ColumnLayout {
                                    id: trowCol
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    anchors.leftMargin: 44
                                    anchors.rightMargin: 14
                                    anchors.topMargin: 12
                                    spacing: 4
                                    Text { text: trow.modelData.email; font.pixelSize: 14; font.weight: Font.DemiBold; color: Theme.textStrong }
                                    Text {
                                        Layout.fillWidth: true
                                        elide: Text.ElideRight
                                        text: trow.modelData.imapHost + ":" + trow.modelData.imapPort + "  ·  " + trow.modelData.smtpHost + ":" + trow.modelData.smtpPort
                                              + (trow.modelData.name ? "  ·  " + trow.modelData.name : "")
                                        font.pixelSize: 12
                                        color: Theme.textMuted
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        wrapMode: Text.Wrap
                                        font.pixelSize: 12
                                        color: trow.status === "ok" || trow.modelData.exists ? Theme.accent
                                             : (trow.status !== "" && trow.status !== "busy") ? Theme.danger : Theme.textSubtle
                                        text: trow.modelData.exists || trow.status === "ok" ? qsTr("Added")
                                            : trow.status === "busy" ? qsTr("Checking with the server…")
                                            : trow.status !== "" ? trow.status
                                            : trow.modelData.hasPassword ? qsTr("Saved password found")
                                            : trow.modelData.oauth ? qsTr("Thunderbird signs in with Google or Microsoft. Enter an app password to use it here.")
                                            : qsTr("No saved password. Enter it below.")
                                    }
                                    TextField {
                                        visible: trow.needsPassword && trow.status !== "ok"
                                        Layout.preferredWidth: 320
                                        echoMode: TextInput.Password
                                        placeholderText: trow.modelData.oauth ? qsTr("App password") : qsTr("Password")
                                        color: Theme.text
                                        onTextEdited: {
                                            const p = Object.assign({}, root.tbPasswords)
                                            p[trow.modelData.email] = text
                                            root.tbPasswords = p
                                        }
                                        background: Rectangle { radius: 8; color: Theme.window; border.color: parent.activeFocus ? Theme.accent : Theme.border }
                                    }
                                }
                            }
                        }
                        RowLayout {
                            spacing: 10
                            Button2 { text: qsTr("Back"); onClicked: root.page = "accounts" }
                            Button2 {
                                objectName: "importTbRun"
                                primary: true
                                enabled: root.tbAccounts.some(a => root.tbPicked[a.email] && !a.exists && root.tbStatus[a.email] !== "busy")
                                text: qsTr("Import selected")
                                onClicked: root.runImport()
                            }
                        }
                    }

                    // ---------------- Add account
                    ColumnLayout {
                        id: form
                        visible: root.page === "addAccount"
                        Layout.fillWidth: true
                        spacing: 12
                        function clear() {
                            fName.text = ""; fEmail.text = ""; fPass.text = ""
                            fImap.text = ""; fImapPort.text = ""; fSmtp.text = ""; fSmtpPort.text = ""; fUser.text = ""
                            root.showManual = false
                        }
                        readonly property string domain: fEmail.text.indexOf("@") > 0 ? fEmail.text.split("@")[1].toLowerCase() : ""
                        readonly property bool isGmail: domain === "gmail.com" || domain === "googlemail.com"
                        readonly property bool isMicrosoft: ["outlook.com", "hotmail.com", "live.com", "msn.com"].indexOf(domain) >= 0
                        Title { text: qsTr("Add account") }
                        Text {
                            Layout.fillWidth: true
                            text: qsTr("Enter your email and password. The mail servers are found automatically; set them yourself under Server settings if your provider needs it.")
                            wrapMode: Text.Wrap
                            font.pixelSize: 13
                            color: Theme.textSubtle
                        }
                        Field { id: fName; label: qsTr("Your name"); placeholder: qsTr("Shown to people you write to") }
                        Field { id: fEmail; objectName: "fieldEmail"; label: qsTr("Email address"); placeholder: "name@example.com" }
                        Field { id: fPass; label: qsTr("Password"); secret: true; placeholder: form.isGmail ? qsTr("Google app password") : "" }
                        Rectangle {
                            visible: form.isGmail || form.isMicrosoft
                            Layout.fillWidth: true
                            implicitHeight: tip.implicitHeight + 20
                            radius: 8
                            color: Theme.selected
                            Text {
                                id: tip
                                anchors.fill: parent
                                anchors.margins: 10
                                wrapMode: Text.Wrap
                                font.pixelSize: 12
                                color: Theme.text
                                text: form.isGmail
                                    ? qsTr("Gmail: use an app password, not your normal password. Create one in your Google Account under Security → 2-Step Verification → App passwords.")
                                    : qsTr("Outlook / Hotmail: use an app password from your Microsoft account security settings, with IMAP enabled.")
                            }
                        }
                        Text {
                            text: (root.showManual ? "▾ " : "▸ ") + qsTr("Server settings")
                            color: Theme.accent
                            font.pixelSize: 13
                            HoverHandler { cursorShape: Qt.PointingHandCursor }
                            TapHandler { onTapped: root.showManual = !root.showManual }
                        }
                        GridLayout {
                            visible: root.showManual
                            Layout.fillWidth: true
                            columns: 3
                            columnSpacing: 10
                            rowSpacing: 10
                            Field { id: fImap; label: qsTr("Incoming (IMAP) server"); placeholder: "mail.example.com" }
                            Field { id: fImapPort; label: qsTr("Port"); placeholder: imapSec.currentValue === "tls" ? "993" : "143"; Layout.preferredWidth: 90; Layout.fillWidth: false }
                            Column {
                                spacing: 5
                                Text { text: qsTr("Security"); font.pixelSize: 12; color: Theme.textSubtle }
                                ComboBox { id: imapSec; textRole: "t"; valueRole: "v"; model: [{ t: "SSL/TLS", v: "tls" }, { t: "STARTTLS", v: "starttls" }, { t: qsTr("None"), v: "none" }] }
                            }
                            Field { id: fSmtp; label: qsTr("Outgoing (SMTP) server"); placeholder: fImap.text || "mail.example.com" }
                            Field { id: fSmtpPort; label: qsTr("Port"); placeholder: smtpSec.currentValue === "tls" ? "465" : "587"; Layout.preferredWidth: 90; Layout.fillWidth: false }
                            Column {
                                spacing: 5
                                Text { text: qsTr("Security"); font.pixelSize: 12; color: Theme.textSubtle }
                                ComboBox { id: smtpSec; textRole: "t"; valueRole: "v"; model: [{ t: "SSL/TLS", v: "tls" }, { t: "STARTTLS", v: "starttls" }, { t: qsTr("None"), v: "none" }] }
                            }
                            Field { id: fUser; label: qsTr("User name"); placeholder: fEmail.text || qsTr("Same as the email address"); Layout.columnSpan: 3 }
                        }
                        Text {
                            visible: root.addError !== ""
                            Layout.fillWidth: true
                            text: root.addError
                            color: Theme.danger
                            wrapMode: Text.Wrap
                            font.pixelSize: 13
                        }
                        RowLayout {
                            spacing: 10
                            Button2 {
                                text: root.busyEmail !== "" ? qsTr("Checking…") : qsTr("Add account")
                                primary: true
                                enabled: root.busyEmail === "" && fEmail.text.indexOf("@") > 0 && fPass.text.length > 0
                                onClicked: {
                                    root.addError = ""
                                    root.busyEmail = fEmail.text
                                    const req = { email: fEmail.text, name: fName.text, password: fPass.text }
                                    if (root.showManual && fImap.text.trim() !== "") {
                                        Object.assign(req, {
                                            imapHost: fImap.text, imapPort: fImapPort.text, imapSecurity: imapSec.currentValue,
                                            smtpHost: fSmtp.text, smtpPort: fSmtpPort.text, smtpSecurity: smtpSec.currentValue,
                                            username: fUser.text
                                        })
                                    }
                                    root.backend.add_account(JSON.stringify(req))
                                }
                            }
                            BusyIndicator { visible: root.busyEmail !== ""; running: visible; implicitWidth: 28; implicitHeight: 28 }
                            Button2 { text: qsTr("Cancel"); onClicked: { root.addError = ""; root.page = "accounts" } }
                        }
                    }

                    // ---------------- Notifications
                    ColumnLayout {
                        visible: root.page === "notifications"
                        Layout.fillWidth: true
                        spacing: 8
                        Title { text: qsTr("Notifications") }
                        Option {
                            label: qsTr("New mail notifications")
                            help: qsTr("Show a desktop notification when new mail arrives. Click it to open the conversation.")
                            Toggle { key: "notifications" }
                        }
                        Option {
                            label: qsTr("Show message preview")
                            help: qsTr("Include the subject and the first lines. Off: only the sender is shown.")
                            enabled: root.prefs.notifications === true
                            opacity: enabled ? 1 : 0.5
                            Toggle { key: "notificationPreview" }
                        }
                        Option {
                            label: qsTr("Unread dot on the status-bar icon")
                            help: qsTr("Mark the top-bar icon while the inbox has unread mail.")
                            Toggle { key: "trayUnreadDot" }
                        }
                    }

                    // ---------------- Appearance
                    ColumnLayout {
                        visible: root.page === "appearance"
                        Layout.fillWidth: true
                        spacing: 8
                        Title { text: qsTr("Appearance") }
                        Option {
                            label: qsTr("Theme")
                            help: qsTr("Follow the system theme (changes with it), or always use light or dark.")
                            Choice { key: "themeMode"; values: ["system", "light", "dark"]; labels: [qsTr("System"), qsTr("Light"), qsTr("Dark")] }
                        }
                        Option {
                            label: qsTr("List density")
                            help: qsTr("Compact fits more conversations on the screen.")
                            Choice { key: "density"; values: ["comfortable", "compact"]; labels: [qsTr("Comfortable"), qsTr("Compact")] }
                        }
                        Option {
                            label: qsTr("Show avatars")
                            help: qsTr("Coloured initials next to each conversation.")
                            Toggle { key: "showAvatars" }
                        }
                        Option {
                            label: qsTr("Show preview text")
                            help: qsTr("A line of the message under the subject.")
                            Toggle { key: "showPreview" }
                        }
                    }

                    // ---------------- Templates
                    ColumnLayout {
                        visible: root.page === "templates"
                        Layout.fillWidth: true
                        spacing: 10
                        Title { text: qsTr("Templates") }
                        Text {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            text: qsTr("Reuse messages you write often. Save one from the composer with the template button, then insert it from the same button.")
                            font.pixelSize: 13
                            color: Theme.textSubtle
                        }
                        Text {
                            visible: root.templates.length === 0
                            text: qsTr("No templates yet")
                            font.pixelSize: 14
                            color: Theme.textMuted
                            topPadding: 10
                        }
                        Repeater {
                            model: root.templates
                            delegate: Rectangle {
                                id: tcard
                                required property var modelData
                                Layout.fillWidth: true
                                implicitHeight: tcol.implicitHeight + 24
                                radius: 10
                                color: Theme.surface
                                border.color: Theme.border
                                ColumnLayout {
                                    id: tcol
                                    anchors.left: parent.left
                                    anchors.right: tdel.left
                                    anchors.verticalCenter: parent.verticalCenter
                                    anchors.margins: 14
                                    spacing: 3
                                    Text { Layout.fillWidth: true; text: tcard.modelData.name; font.pixelSize: 14; font.weight: Font.DemiBold; color: Theme.textStrong; elide: Text.ElideRight }
                                    Text {
                                        Layout.fillWidth: true
                                        text: (tcard.modelData.subject ? tcard.modelData.subject + " · " : "") + String(tcard.modelData.html).replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim()
                                        font.pixelSize: 12
                                        color: Theme.textMuted
                                        elide: Text.ElideRight
                                    }
                                }
                                Button2 {
                                    id: tdel
                                    anchors.right: parent.right
                                    anchors.rightMargin: 12
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: qsTr("Delete")
                                    danger: true
                                    onClicked: { root.backend.delete_template(tcard.modelData.id); root.reload() }
                                }
                            }
                        }
                    }

                    // ---------------- Shortcuts
                    ColumnLayout {
                        visible: root.page === "shortcuts"
                        Layout.fillWidth: true
                        spacing: 4
                        Title { text: qsTr("Keyboard shortcuts") }
                        Repeater {
                            model: [
                                ["J / ↓", qsTr("Next conversation")], ["K / ↑", qsTr("Previous conversation")],
                                ["Enter / O", qsTr("Open")], ["Esc", qsTr("Close / cancel selection")],
                                ["X", qsTr("Select the focused conversation")], ["Ctrl+A", qsTr("Select all")],
                                ["Shift+click", qsTr("Select a range")], ["Ctrl+click", qsTr("Add to the selection")],
                                ["E", qsTr("Done / not done")], ["Shift+E", qsTr("Show or hide Done mail in the Inbox")], ["P", qsTr("Pin / unpin")], ["S", qsTr("Snooze")],
                                ["U", qsTr("Mark read / unread")], ["#, Delete", qsTr("Delete")], ["!", qsTr("Spam")],
                                ["Z / Ctrl+Z", qsTr("Undo")], ["Ctrl+,", qsTr("Settings")],
                                ["C / Ctrl+N", qsTr("New message")], ["R", qsTr("Reply")], ["A", qsTr("Reply all")],
                                ["F", qsTr("Forward")], ["Ctrl+Enter", qsTr("Send")], ["Ctrl+Shift+A", qsTr("Attach files")]
                            ]
                            delegate: RowLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                spacing: 14
                                Rectangle {
                                    Layout.preferredWidth: 120
                                    implicitHeight: 28
                                    radius: 6
                                    color: Theme.surface
                                    border.color: Theme.border
                                    Text { anchors.centerIn: parent; text: modelData[0]; font.pixelSize: 12; font.weight: Font.DemiBold; color: Theme.text }
                                }
                                Text { Layout.fillWidth: true; text: modelData[1]; font.pixelSize: 14; color: Theme.text }
                            }
                        }
                    }

                    // ---------------- Updates
                    ColumnLayout {
                        visible: root.page === "updates"
                        Layout.fillWidth: true
                        spacing: 8
                        Title { text: qsTr("Updates") }

                        // current version + status + the main action
                        Rectangle {
                            objectName: "updateCard"
                            Layout.fillWidth: true
                            implicitHeight: updCol.implicitHeight + 32
                            radius: 10
                            color: Theme.surface
                            ColumnLayout {
                                id: updCol
                                x: 16; y: 16
                                width: parent.width - 32
                                spacing: 10
                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 14
                                    Image {
                                        Layout.preferredWidth: 44
                                        Layout.preferredHeight: 44
                                        sourceSize: Qt.size(88, 88)
                                        source: "qrc:/qt/qml/org/rustyinbox/icons/app.png"
                                    }
                                    Column {
                                        Layout.fillWidth: true
                                        spacing: 3
                                        Text {
                                            text: root.backend.app_name() + " " + (root.upd.current || "")
                                            font.pixelSize: 16
                                            font.weight: Font.DemiBold
                                            color: Theme.textStrong
                                        }
                                        Text {
                                            width: parent.width
                                            wrapMode: Text.Wrap
                                            font.pixelSize: 13
                                            color: root.upd.state === "error" ? Theme.danger
                                                 : root.upd.state === "available" || root.upd.state === "ready" ? Theme.accent : Theme.textMuted
                                            text: {
                                                const u = root.upd, v = u.latest ? u.latest.version : ""
                                                switch (u.state) {
                                                case "checking": return qsTr("Checking for updates…")
                                                case "current": return qsTr("You have the latest version.")
                                                case "available": return qsTr("Version %1 is available.").arg(v)
                                                case "downloading": return u.total > 0
                                                    ? qsTr("Downloading %1… %2 of %3 MB").arg(v).arg(root.fmtMb(u.received)).arg(root.fmtMb(u.total))
                                                    : qsTr("Downloading %1…").arg(v)
                                                case "ready": return u.restart ? qsTr("Version %1 is installed. Restart to start using it.").arg(v)
                                                                 : u.quit ? qsTr("The installer is updating %1 now.").arg(root.backend.app_name())
                                                                 : qsTr("Version %1 is downloaded. Open it and drag the app into Applications to finish.").arg(v)
                                                case "error": return qsTr("Could not update: %1").arg(u.error)
                                                default: return qsTr("Not checked yet.")
                                                }
                                            }
                                        }
                                    }
                                    Button2 {
                                        objectName: "checkUpdateButton"
                                        visible: root.upd.state !== "ready" && root.upd.state !== "downloading"
                                        enabled: !root.updBusy
                                        text: qsTr("Check now")
                                        onClicked: root.backend.check_update()
                                    }
                                    Button2 {
                                        objectName: "installUpdateButton"
                                        visible: root.upd.state === "available" && root.upd.method && root.upd.method.canInstall
                                        text: root.upd.method && root.upd.method.inPlace ? qsTr("Download and install") : qsTr("Download")
                                        primary: true
                                        onClicked: root.backend.install_update()
                                    }
                                    Button2 {
                                        visible: root.upd.state === "ready" && root.upd.restart === true
                                        text: qsTr("Restart now")
                                        primary: true
                                        onClicked: if (root.backend.restart_into_update()) Qt.quit()
                                    }
                                }
                                // download progress
                                Rectangle {
                                    visible: root.upd.state === "downloading"
                                    Layout.fillWidth: true
                                    implicitHeight: 6
                                    radius: 3
                                    color: Theme.border
                                    Rectangle {
                                        height: parent.height
                                        radius: 3
                                        color: Theme.accent
                                        width: root.upd.total > 0 ? parent.width * Math.min(1, root.upd.received / root.upd.total) : 0
                                        Behavior on width { NumberAnimation { duration: 150 } }
                                    }
                                }
                                // install types the app cannot update by itself
                                Text {
                                    visible: root.upd.state === "available" && root.upd.method && !root.upd.method.canInstall
                                    Layout.fillWidth: true
                                    wrapMode: Text.Wrap
                                    textFormat: Text.MarkdownText
                                    font.pixelSize: 13
                                    color: Theme.text
                                    text: root.upd.method ? root.plainCode(root.upd.method.howTo) : ""
                                }
                                Text {
                                    Layout.fillWidth: true
                                    font.pixelSize: 12
                                    color: Theme.textMuted
                                    text: qsTr("Installed as: %1").arg(root.upd.method ? root.upd.method.label : "")
                                          + (root.upd.lastCheck > 0 ? "  ·  " + qsTr("Last checked %1").arg(new Date(root.upd.lastCheck).toLocaleString(Qt.locale(), Locale.ShortFormat)) : "")
                                }
                            }
                        }

                        // what's new in the release found
                        Group {
                            visible: !!root.upd.latest && root.upd.state !== "current" && root.upd.state !== "checking"
                            text: root.upd.latest ? qsTr("What's new in %1").arg(root.upd.latest.version) : ""
                        }
                        Rectangle {
                            objectName: "releaseNotes"
                            visible: !!root.upd.latest && root.upd.state !== "current" && root.upd.state !== "checking"
                            Layout.fillWidth: true
                            implicitHeight: notesCol.implicitHeight + 32
                            radius: 10
                            color: Theme.surface
                            Column {
                                id: notesCol
                                x: 16; y: 16
                                width: parent.width - 32
                                spacing: 10
                                Text {
                                    text: root.upd.latest ? root.upd.latest.name + "  ·  " + root.fmtDate(root.upd.latest.published) : ""
                                    font.pixelSize: 14
                                    font.weight: Font.DemiBold
                                    color: Theme.textStrong
                                }
                                Text {
                                    width: parent.width
                                    wrapMode: Text.Wrap
                                    textFormat: Text.MarkdownText
                                    font.pixelSize: 13
                                    color: Theme.text
                                    linkColor: Theme.accent
                                    text: root.upd.latest ? root.plainCode(root.upd.latest.notes) : ""
                                    onLinkActivated: (link) => Qt.openUrlExternally(link)
                                    HoverHandler { cursorShape: parent.hoveredLink ? Qt.PointingHandCursor : Qt.ArrowCursor }
                                }
                                Button2 {
                                    text: qsTr("Open the release page")
                                    onClicked: Qt.openUrlExternally(root.upd.latest.url || root.upd.releasesUrl)
                                }
                            }
                        }

                        Group { text: qsTr("Automatic updates") }
                        Option {
                            label: qsTr("Check for updates automatically")
                            help: qsTr("Looks for a new version in the background and tells you when one is out.")
                            Toggle { key: "autoUpdateCheck" }
                        }
                        Option {
                            label: qsTr("How often")
                            enabled: root.prefs.autoUpdateCheck === true
                            opacity: enabled ? 1 : 0.5
                            Choice { key: "updateCheckDays"; values: [1, 7]; labels: [qsTr("Every day"), qsTr("Every week")] }
                        }
                        Option {
                            label: qsTr("Install updates automatically")
                            help: root.upd.method && root.upd.method.inPlace
                                  ? qsTr("Downloads and installs new versions in the background. They start the next time you open the app.")
                                  : qsTr("Available for the AppImage and Linux binary installs. This copy is updated as: %1.").arg(root.upd.method ? root.upd.method.label : "")
                            enabled: root.prefs.autoUpdateCheck === true && !!root.upd.method && root.upd.method.inPlace && root.upd.method.canInstall
                            opacity: enabled ? 1 : 0.5
                            Toggle { key: "autoInstallUpdates" }
                        }
                    }

                    // ---------------- AI (qml/AiSettingsPage.qml)
                    AiSettingsPage {
                        id: aiSettingsPage
                        visible: root.page === "ai"
                        Layout.fillWidth: true
                        host: root
                    }

                    // ---------------- About
                    ColumnLayout {
                        visible: root.page === "about"
                        Layout.fillWidth: true
                        spacing: 10
                        Image {
                            Layout.preferredWidth: 96
                            Layout.preferredHeight: 96
                            sourceSize: Qt.size(192, 192)
                            source: "qrc:/qt/qml/org/rustyinbox/icons/app.png"
                        }
                        Title { text: root.backend.app_name() }
                        Text { text: qsTr("Version %1").arg(root.backend.version()); color: Theme.textMuted; font.pixelSize: 13 }
                        Text {
                            Layout.fillWidth: true
                            text: qsTr("A fast, native email client. The mail engine is written in Rust; the interface is Qt / QML and follows your system theme.")
                            wrapMode: Text.Wrap
                            font.pixelSize: 14
                            color: Theme.text
                        }
                    }
                }
            }
        }
    }
}
