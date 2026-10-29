import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import org.rustyinbox

// Settings → AI: master switch, provider cards with their connect flows, live model picker, thinking
// level, test connection, answer language, feature switches, excluded accounts. WORKER B owns this
// file; the data comes from the Rust object `AiSettings` (app/src/ai_settings.rs) as JSON.
// `host` is the Settings window (prefs, accounts, save(), reload(), notice()).
ColumnLayout {
    id: pg
    property var host
    property bool openLinks: true             // the scripted check turns this off
    property var st: ({ providers: [], modelThinking: [] })
    spacing: 8

    // ---- model list / picker state
    property string modelsState: "idle"       // idle | loading | ready | error
    property string modelsError: ""
    property var modelRows: []
    property var rowsMeta: ({ rows: [], total: 0, shown: 0, exact: false })
    // ---- test connection
    property string testState: "idle"         // idle | busy | ok | error
    property string testReply: ""
    property string testError: ""
    property real testSeconds: 0
    // ---- ChatGPT sign-in
    property string signState: "idle"         // idle | starting | browser | device | failed
    property string signUrl: ""
    property string signCode: ""
    property string signError: ""

    readonly property string provider: st.provider || ""
    readonly property var cur: st.providers.find(p => p.id === provider) || null
    readonly property string providerName: st.providerLabel ? st.providerLabel : qsTr("your AI provider")
    readonly property bool ready: !!cur && cur.configured && (st.model || "") !== ""
    readonly property bool featuresOn: host.prefs.aiEnabled === true
    readonly property var levelNames: ({ off: qsTr("Off"), on: qsTr("On"), minimal: qsTr("Minimal"), low: qsTr("Low"), medium: qsTr("Medium"), high: qsTr("High"), xhigh: qsTr("Extra high"), max: qsTr("Max"), ultra: qsTr("Ultra") })

    AiSettings { id: eng }

    function refresh() { st = JSON.parse(eng.state_json()) }
    // run after a call that changed settings
    function act(err) {
        if (err && err !== "") host.notice(err)
        host.reload()
        refresh()
    }
    function loadModels(force) {
        if (!provider || !cur || !cur.configured) return
        if (!force && st.modelsLoaded) { modelsState = "ready"; return }
        modelsState = "loading"
        eng.start_model_list(provider)
    }
    function choose(id) {
        if (id === provider) return
        modelsState = "idle"; modelsError = ""; testState = "idle"
        act(eng.choose_provider(id))
        loadModels(false)
    }
    function afterKey(id) {
        act("")
        testState = "idle"
        if (id === provider) loadModels(true)
    }
    function runSearch() {
        const typed = search.text.trim()
        const d = JSON.parse(eng.search_models(provider, search.text, 200))
        const rows = d.rows.slice()
        if (typed !== "" && !d.exact) rows.push({ use: true, id: typed })
        rowsMeta = d
        modelRows = rows
        list.currentIndex = rows.length ? 0 : -1
    }
    function pickModel(id) {
        const err = eng.choose_model(id)
        pop.close()
        testState = "idle"
        act(err)
    }
    function esc(s) { return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;") }
    // text with the matched letters (char positions from Rust) in the accent colour
    function hl(text, idx) {
        const chars = Array.from(text || "")
        if (!idx || !idx.length) return esc(chars.join(""))
        const on = {}
        for (const i of idx) on[i] = true
        let out = "", open = false
        chars.forEach((c, i) => {
            if (on[i] && !open) { out += "<font color=\"" + Theme.accent + "\"><b>"; open = true }
            if (!on[i] && open) { out += "</b></font>"; open = false }
            out += esc(c)
        })
        return open ? out + "</b></font>" : out
    }
    function excludedList() { return (host.prefs.aiExcluded || "").split(",").map(x => x.trim()).filter(x => x !== "") }
    function toggleExcluded(email) {
        const l = excludedList()
        const i = l.findIndex(x => x.toLowerCase() === email.toLowerCase())
        if (i >= 0) l.splice(i, 1); else l.push(email)
        host.save("aiExcluded", l.join(","))
    }
    function isExcluded(email) { return excludedList().some(x => x.toLowerCase() === email.toLowerCase()) }
    function startSignIn(device) {
        signState = "starting"; signError = ""
        if (device) eng.use_device_code(); else eng.sign_in_chatgpt()
    }
    function cancelSignIn() { eng.cancel_sign_in(); signState = "idle"; signError = "" }
    function copyText(t) { clip.text = t; clip.selectAll(); clip.copy(); host.notice(qsTr("Copied")) }
    // scripted check: show a sign-in state without a network
    function fakeSignIn(kind) {
        signUrl = kind === "device" ? "auth.openai.com/codex/device" : "https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_EMoamEEZ73f0CkXaXp7hrann&state=demo"
        signCode = "ABCD-1234"
        signError = qsTr("Port 1455 is busy.")
        signState = kind
    }
    // scripted check (Main.qml, AI(B)): drive the page the way a click would
    function autotestUse(id, url) {
        choose(id)
        if (url) { act(eng.set_base_url(id, url)); modelsState = "idle"; loadModels(true) }
    }
    function autotestPopup() { pop.open(); const o = pop.visible; pop.close(); return o }
    function autotestOpenPicker(query) {
        if (!inlineHost.visible) {
            pop.close()
            panel.parent = inlineHost
            panel.width = inlineHost.width
            panel.height = inlineHost.height
            inlineHost.visible = true
        }
        search.text = query
        loadModels(false)
        runSearch()
    }
    function autotestScroll(name, above) {
        const m = { provider: grpProvider, model: grpModel, thinking: thinkCard, test: testCard, language: grpLanguage, writing: grpWriting, organize: grpOrganize, excluded: grpExcluded, privacy: grpPrivacy }
        host.scrollToItem(m[name], above || 0)
    }
    function autotestClosePicker() { inlineHost.visible = false }
    function autotestThinking(l) { act(eng.choose_thinking(l)) }
    function autotestTest() { testState = "busy"; eng.test_connection() }
    function fmtSeconds(s) { return s < 10 ? s.toFixed(1) + " s" : Math.round(s) + " s" }

    Component.onCompleted: { refresh(); loadModels(false) }
    onVisibleChanged: if (visible) { refresh(); loadModels(false) }
    Connections {
        target: pg.host
        function onPrefsChanged() { pg.refresh() }
    }
    Connections {
        target: eng
        function onModels_loaded(p, json, error) {
            if (p !== pg.provider) return
            pg.refresh()
            if (error !== "") { pg.modelsState = "error"; pg.modelsError = error } else { pg.modelsState = "ready"; pg.modelsError = "" }
            if (pop.opened || inlineHost.visible) pg.runSearch()
        }
        function onTest_done(ok, reply, millis, error) {
            pg.testState = ok ? "ok" : "error"
            pg.testReply = reply
            pg.testError = error
            pg.testSeconds = millis / 1000
        }
        function onSign_in_url(url) {
            pg.signUrl = url; pg.signState = "browser"
            if (pg.openLinks) Qt.openUrlExternally(url)
        }
        function onSign_in_code(url, code) {
            pg.signUrl = url; pg.signCode = code; pg.signState = "device"
            if (pg.openLinks) Qt.openUrlExternally(url)
        }
        function onSign_in_done(json) {
            pg.signState = "idle"
            pg.act("")
            pg.loadModels(true)
        }
        function onSign_in_failed(message) { pg.signState = "failed"; pg.signError = message }
    }
    TextEdit { id: clip; visible: false }

    // ---- building blocks (same look as Settings.qml) ----------------------------------------------

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
    component Spinner: IconImage {
        width: 16; height: 16
        sourceSize: Qt.size(32, 32)
        source: "qrc:/qt/qml/org/rustyinbox/icons/update.svg"
        color: Theme.icon
        RotationAnimator on rotation { from: 0; to: 360; duration: 900; loops: Animation.Infinite; running: parent.visible }
    }
    // label + help (+ a plain note about what is sent) on the left, the control on the right
    component AiOption: Rectangle {
        id: opt
        property string label
        property string help
        property string note              // "Sends mail content to ..." in a stronger tone
        default property alias control: slot.data
        Layout.fillWidth: true
        implicitHeight: Math.max(56, textCol.implicitHeight + 22)
        radius: 10
        color: Theme.surface
        opacity: enabled ? 1 : 0.5
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
            Text {
                width: parent.width
                visible: opt.note !== ""
                text: opt.note
                font.pixelSize: 12
                font.weight: Font.DemiBold
                color: Theme.textSubtle
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
    component AiToggle: Rectangle {
        id: tg
        property string key
        readonly property bool on: pg.host.prefs[key] === true
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
        TapHandler { onTapped: if (tg.enabled) pg.host.save(tg.key, !tg.on) }
    }
    // segmented choice: a Settings value (key) or a free selection (current + picked())
    component AiChoice: Row {
        id: ch
        property string key
        property var values: []
        property var labels: []
        property var current: pg.host.prefs[key]
        signal picked(var value)
        spacing: 0
        Repeater {
            model: ch.values.length
            delegate: Rectangle {
                required property int index
                readonly property bool on: ch.current === ch.values[index]
                width: lab.implicitWidth + 22
                height: 30
                radius: 7
                color: on ? Theme.accent : (h.hovered ? Theme.hover : "transparent")
                border.color: on ? Theme.accent : Theme.border
                Behavior on color { ColorAnimation { duration: Theme.fast } }
                Text {
                    id: lab
                    anchors.centerIn: parent
                    text: ch.labels[index] !== undefined ? ch.labels[index] : ""
                    font.pixelSize: 13
                    color: parent.on ? Theme.accentText : Theme.text
                }
                HoverHandler { id: h; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: { if (ch.key !== "") pg.host.save(ch.key, ch.values[index]); ch.picked(ch.values[index]) } }
            }
        }
    }
    component AiButton: Rectangle {
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
    component AiField: Column {
        id: fld
        property string label
        property alias text: input.text
        property alias placeholder: input.placeholderText
        property bool secret: false
        property alias input: input
        signal accepted()
        Layout.fillWidth: true
        spacing: 5
        Text { text: fld.label; font.pixelSize: 12; color: Theme.textSubtle }
        TextField {
            id: input
            width: parent.width
            echoMode: fld.secret ? TextInput.Password : TextInput.Normal
            color: Theme.text
            placeholderTextColor: Theme.textMuted
            font.pixelSize: 14
            selectByMouse: true
            onAccepted: fld.accepted()
            background: Rectangle {
                radius: 8
                color: Theme.window
                border.color: input.activeFocus ? Theme.accent : Theme.border
                border.width: input.activeFocus ? 1.5 : 1
            }
        }
    }
    // an API key: saved once, then only "ends in 1234"
    component KeyBox: ColumnLayout {
        id: kb
        property var entry
        property string fieldLabel: qsTr("API key")
        property string help: qsTr("Stored in this computer's secret store. It is never shown again, only its last 4 characters.")
        property bool replacing: false
        property string error: ""
        Layout.fillWidth: true
        spacing: 8
        function save() {
            const err = eng.set_key(entry.id, keyField.text)
            if (err !== "") { error = err; return }
            keyField.text = ""; error = ""; replacing = false
            pg.afterKey(entry.id)
        }
        RowLayout {
            visible: kb.entry.hasKey && !kb.replacing
            Layout.fillWidth: true
            spacing: 10
            Text {
                Layout.fillWidth: true
                text: kb.entry.keyHint !== "" ? qsTr("Key saved. It ends in %1.").arg(kb.entry.keyHint) : qsTr("Key saved.")
                font.pixelSize: 13
                color: Theme.text
            }
            AiButton { text: qsTr("Replace"); onClicked: kb.replacing = true }
            AiButton { text: qsTr("Remove"); danger: true; onClicked: { eng.clear_key(kb.entry.id); pg.afterKey(kb.entry.id) } }
        }
        ColumnLayout {
            visible: !kb.entry.hasKey || kb.replacing
            Layout.fillWidth: true
            spacing: 8
            AiField { id: keyField; label: kb.fieldLabel; secret: true; placeholder: qsTr("Paste the key"); onAccepted: kb.save() }
            Text { Layout.fillWidth: true; text: kb.help; font.pixelSize: 12; color: Theme.textMuted; wrapMode: Text.Wrap }
            Text { visible: kb.error !== ""; Layout.fillWidth: true; text: kb.error; font.pixelSize: 12; color: Theme.danger; wrapMode: Text.Wrap }
            RowLayout {
                spacing: 8
                AiButton { text: qsTr("Save key"); primary: true; enabled: keyField.text.trim() !== ""; onClicked: kb.save() }
                AiButton { visible: kb.replacing; text: qsTr("Cancel"); onClicked: { kb.replacing = false; keyField.text = ""; kb.error = "" } }
            }
        }
    }

    // ---- page -------------------------------------------------------------------------------------

    Title { text: qsTr("AI") }
    Text {
        Layout.fillWidth: true
        text: qsTr("Optional help with writing, reading and sorting mail. You choose the provider and the model, and nothing is sent anywhere until you turn this on.")
        wrapMode: Text.Wrap
        font.pixelSize: 13
        color: Theme.textSubtle
        bottomPadding: 6
    }
    AiOption {
        objectName: "aiMaster"
        label: qsTr("Use AI")
        help: qsTr("Off: nothing leaves this computer. On: only the text a feature needs is sent, to the provider you pick below, and only when you click it or switch the feature on. Accounts you exclude are never sent.")
        AiToggle { key: "aiEnabled" }
    }
    Rectangle {
        visible: pg.host.prefs.aiEnabled === true && !pg.ready
        Layout.fillWidth: true
        implicitHeight: setupNote.implicitHeight + 22
        radius: 10
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
        Text {
            id: setupNote
            x: 16; y: 11
            width: parent.width - 32
            wrapMode: Text.Wrap
            font.pixelSize: 13
            color: Theme.text
            text: !pg.cur ? qsTr("Choose a provider below to finish setting up.")
                : !pg.cur.configured ? qsTr("Connect %1 below, then choose a model.").arg(pg.providerName)
                : qsTr("Choose a model below to finish setting up.")
        }
    }

    // ---- providers
    Group { id: grpProvider; text: qsTr("Provider") }
    Repeater {
        model: pg.st.providers
        delegate: Rectangle {
            id: card
            required property var modelData
            readonly property bool sel: pg.provider === modelData.id
            Layout.fillWidth: true
            implicitHeight: cardCol.implicitHeight + 28
            radius: 10
            color: Theme.surface
            border.color: sel ? Theme.accent : "transparent"
            border.width: sel ? 1.5 : 0
            Behavior on border.color { ColorAnimation { duration: Theme.fast } }
            ColumnLayout {
                id: cardCol
                x: 16; y: 14
                width: parent.width - 32
                spacing: 12
                Item {
                    Layout.fillWidth: true
                    implicitHeight: head.implicitHeight
                    RowLayout {
                        id: head
                        anchors.left: parent.left
                        anchors.right: parent.right
                        spacing: 12
                        Rectangle {
                            width: 20; height: 20; radius: 10
                            color: "transparent"
                            border.width: 1.6
                            border.color: card.sel ? Theme.accent : Theme.textMuted
                            Rectangle {
                                anchors.centerIn: parent
                                width: 10; height: 10; radius: 5
                                color: Theme.accent
                                opacity: card.sel ? 1 : 0
                                scale: card.sel ? 1 : 0.4
                                Behavior on opacity { NumberAnimation { duration: Theme.fast } }
                                Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }
                            }
                        }
                        Column {
                            Layout.fillWidth: true
                            spacing: 2
                            Text { text: card.modelData.label; font.pixelSize: 15; font.weight: Font.DemiBold; color: Theme.textStrong }
                            Text { width: parent.width; text: card.modelData.blurb; font.pixelSize: 12; color: Theme.textMuted; wrapMode: Text.Wrap }
                        }
                        Rectangle {
                            Layout.alignment: Qt.AlignVCenter
                            implicitWidth: pillRow.implicitWidth + 20
                            implicitHeight: 24
                            radius: 12
                            color: card.modelData.connected ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.14) : Theme.border
                            Row {
                                id: pillRow
                                anchors.centerIn: parent
                                spacing: 6
                                Rectangle {
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: 7; height: 7; radius: 4
                                    color: card.modelData.connected ? Theme.accent : Theme.textMuted
                                }
                                Text {
                                    text: card.modelData.connected ? qsTr("Connected") : qsTr("Not connected")
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    color: card.modelData.connected ? Theme.accent : Theme.textSubtle
                                }
                            }
                        }
                    }
                    HoverHandler { cursorShape: Qt.PointingHandCursor }
                    TapHandler { onTapped: pg.choose(card.modelData.id) }
                }

                // ChatGPT subscription: a sign-in, no key
                ColumnLayout {
                    visible: card.sel && card.modelData.auth === "chatgpt"
                    Layout.fillWidth: true
                    spacing: 10
                    // signed in
                    RowLayout {
                        visible: !!pg.st.account
                        Layout.fillWidth: true
                        spacing: 10
                        Column {
                            Layout.fillWidth: true
                            spacing: 2
                            Text { text: pg.st.account ? qsTr("Signed in as %1").arg(pg.st.account.email) : ""; font.pixelSize: 13; color: Theme.text }
                            Text {
                                visible: !!pg.st.account && pg.st.account.plan !== ""
                                text: pg.st.account ? qsTr("ChatGPT %1").arg(pg.st.account.plan.charAt(0).toUpperCase() + pg.st.account.plan.slice(1)) : ""
                                font.pixelSize: 12
                                color: Theme.textMuted
                            }
                        }
                        AiButton { text: qsTr("Sign out"); danger: true; onClicked: { eng.sign_out_chatgpt(); pg.signState = "idle"; pg.modelsState = "idle"; pg.act("") } }
                    }
                    // not signed in
                    ColumnLayout {
                        visible: !pg.st.account
                        Layout.fillWidth: true
                        spacing: 10
                        Text {
                            visible: pg.signState === "idle" || pg.signState === "failed"
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            font.pixelSize: 12
                            color: Theme.textMuted
                            text: qsTr("Uses the subscription you already pay for; no API key and no extra charge. Your browser opens so you can sign in to ChatGPT. Your password is never seen by this app; only a sign-in token is kept on this computer, and Sign out removes it.")
                        }
                        Text {
                            visible: pg.signState === "failed"
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            font.pixelSize: 13
                            color: Theme.danger
                            text: pg.signError
                        }
                        RowLayout {
                            visible: pg.signState === "idle" || pg.signState === "failed"
                            spacing: 8
                            AiButton { objectName: "chatgptSignIn"; text: pg.signState === "failed" ? qsTr("Try again") : qsTr("Sign in with ChatGPT"); primary: true; onClicked: pg.startSignIn(false) }
                            AiButton { text: qsTr("Use a code instead"); onClicked: pg.startSignIn(true) }
                        }
                        // starting
                        RowLayout {
                            visible: pg.signState === "starting"
                            spacing: 10
                            Spinner {}
                            Text { text: qsTr("Starting the sign-in…"); font.pixelSize: 13; color: Theme.text }
                            AiButton { text: qsTr("Cancel"); onClicked: pg.cancelSignIn() }
                        }
                        // waiting for the browser
                        ColumnLayout {
                            objectName: "signInBrowser"
                            visible: pg.signState === "browser"
                            Layout.fillWidth: true
                            spacing: 8
                            RowLayout {
                                spacing: 10
                                Spinner {}
                                Text { text: qsTr("Waiting for you to finish signing in in the browser…"); font.pixelSize: 13; color: Theme.text }
                            }
                            Text {
                                Layout.fillWidth: true
                                wrapMode: Text.Wrap
                                font.pixelSize: 12
                                color: Theme.textMuted
                                text: qsTr("If no window opened, copy the link into your browser. This page continues by itself when you are done.")
                            }
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: 34
                                radius: 8
                                color: Theme.window
                                border.color: Theme.border
                                Text {
                                    anchors.fill: parent
                                    anchors.leftMargin: 10; anchors.rightMargin: 10
                                    verticalAlignment: Text.AlignVCenter
                                    elide: Text.ElideMiddle
                                    text: pg.signUrl
                                    font.pixelSize: 12
                                    color: Theme.textSubtle
                                }
                            }
                            RowLayout {
                                spacing: 8
                                AiButton { text: qsTr("Open the link"); primary: true; onClicked: Qt.openUrlExternally(pg.signUrl) }
                                AiButton { text: qsTr("Copy link"); onClicked: pg.copyText(pg.signUrl) }
                                AiButton { text: qsTr("Use a code instead"); onClicked: pg.startSignIn(true) }
                                AiButton { text: qsTr("Cancel"); onClicked: pg.cancelSignIn() }
                            }
                        }
                        // device code
                        ColumnLayout {
                            objectName: "signInDevice"
                            visible: pg.signState === "device"
                            Layout.fillWidth: true
                            spacing: 8
                            Text {
                                Layout.fillWidth: true
                                wrapMode: Text.Wrap
                                font.pixelSize: 13
                                color: Theme.text
                                text: qsTr("Open %1 on any device and type this code:").arg(pg.signUrl)
                            }
                            Rectangle {
                                implicitWidth: codeText.implicitWidth + 40
                                implicitHeight: 52
                                radius: 10
                                color: Theme.window
                                border.color: Theme.border
                                Text {
                                    id: codeText
                                    anchors.centerIn: parent
                                    text: pg.signCode
                                    font.pixelSize: 26
                                    font.weight: Font.DemiBold
                                    font.letterSpacing: 3
                                    color: Theme.textStrong
                                }
                            }
                            RowLayout {
                                spacing: 10
                                Spinner {}
                                Text { text: qsTr("Waiting for you to enter the code…"); font.pixelSize: 13; color: Theme.textSubtle }
                            }
                            RowLayout {
                                spacing: 8
                                AiButton { text: qsTr("Open the page"); primary: true; onClicked: Qt.openUrlExternally(pg.signUrl) }
                                AiButton { text: qsTr("Copy code"); onClicked: pg.copyText(pg.signCode) }
                                AiButton { text: qsTr("Cancel"); onClicked: pg.cancelSignIn() }
                            }
                        }
                    }
                }

                // API key providers
                KeyBox {
                    visible: card.sel && card.modelData.auth === "key"
                    entry: card.modelData
                }

                // Ollama: this computer or ollama.com
                ColumnLayout {
                    visible: card.sel && card.modelData.id === "ollama"
                    Layout.fillWidth: true
                    spacing: 10
                    RowLayout {
                        spacing: 12
                        Text { text: qsTr("Where it runs"); font.pixelSize: 13; color: Theme.text }
                        AiChoice {
                            objectName: "ollamaWhere"
                            values: ["local", "cloud"]
                            labels: [qsTr("This computer"), qsTr("ollama.com")]
                            current: card.modelData.baseUrl.indexOf("ollama.com") >= 0 ? "cloud" : "local"
                            onPicked: (v) => { pg.act(eng.set_base_url("ollama", v === "cloud" ? "https://ollama.com" : "http://localhost:11434")); pg.loadModels(true) }
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        AiField { id: ollamaUrl; Layout.fillWidth: true; label: qsTr("Address"); text: card.modelData.baseUrl; placeholder: "http://localhost:11434"; onAccepted: ollamaSave.clicked() }
                        AiButton { id: ollamaSave; Layout.alignment: Qt.AlignBottom; text: qsTr("Save"); enabled: ollamaUrl.text.trim() !== card.modelData.baseUrl; onClicked: { pg.act(eng.set_base_url("ollama", ollamaUrl.text)); pg.loadModels(true) } }
                    }
                    KeyBox {
                        entry: card.modelData
                        fieldLabel: qsTr("API key (only for ollama.com)")
                        help: qsTr("Not needed for Ollama on this computer. Create a key at ollama.com/settings/keys. It is stored in this computer's secret store and never shown again.")
                    }
                }

                // Custom endpoint
                ColumnLayout {
                    visible: card.sel && card.modelData.id === "custom"
                    Layout.fillWidth: true
                    spacing: 10
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        AiField { id: customUrl; objectName: "customUrl"; Layout.fillWidth: true; label: qsTr("Base URL"); text: card.modelData.baseUrl; placeholder: "http://localhost:1234/v1"; onAccepted: customSave.clicked() }
                        AiButton { id: customSave; Layout.alignment: Qt.AlignBottom; text: qsTr("Save"); enabled: customUrl.text.trim() !== card.modelData.baseUrl; onClicked: { pg.act(eng.set_base_url("custom", customUrl.text)); pg.modelsState = "idle"; pg.loadModels(true) } }
                    }
                    KeyBox {
                        entry: card.modelData
                        fieldLabel: qsTr("API key (optional)")
                        help: qsTr("Only if the server asks for one. It is stored in this computer's secret store and never shown again.")
                    }
                }
            }
        }
    }

    // ---- model
    Group { id: grpModel; text: qsTr("Model"); visible: pg.provider !== "" }
    Rectangle {
        id: modelCard
        visible: pg.provider !== ""
        Layout.fillWidth: true
        implicitHeight: modelCol.implicitHeight + 28
        radius: 10
        color: Theme.surface
        ColumnLayout {
            id: modelCol
            x: 16; y: 14
            width: parent.width - 32
            spacing: 10
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.pixelSize: 12
                color: Theme.textMuted
                text: qsTr("The list comes straight from %1, so it is always current. Type to search: the letters only need to appear in order, so “son4” finds “claude-sonnet-4”.").arg(pg.providerName)
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Rectangle {
                    id: picker
                    objectName: "modelPicker"
                    Layout.fillWidth: true
                    implicitHeight: 44
                    radius: 8
                    color: Theme.window
                    border.color: pop.opened || pickHover.hovered ? Theme.accent : Theme.border
                    border.width: pop.opened ? 1.5 : 1
                    opacity: pg.cur && pg.cur.configured ? 1 : 0.5
                    Behavior on border.color { ColorAnimation { duration: Theme.fast } }
                    Column {
                        anchors.left: parent.left; anchors.leftMargin: 12
                        anchors.right: chev.left; anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 1
                        Text {
                            width: parent.width
                            elide: Text.ElideRight
                            font.pixelSize: 14
                            color: pg.st.model ? Theme.text : Theme.textMuted
                            text: pg.st.model ? pg.st.modelName : qsTr("Choose a model")
                        }
                        Text {
                            visible: !!pg.st.model && pg.st.modelName !== pg.st.model
                            width: parent.width
                            elide: Text.ElideRight
                            font.pixelSize: 11
                            color: Theme.textMuted
                            text: pg.st.model || ""
                        }
                    }
                    IconImage {
                        id: chev
                        anchors.right: parent.right; anchors.rightMargin: 12
                        anchors.verticalCenter: parent.verticalCenter
                        width: 16; height: 16
                        sourceSize: Qt.size(32, 32)
                        source: "qrc:/qt/qml/org/rustyinbox/icons/chevron-down.svg"
                        color: Theme.icon
                        rotation: pop.opened ? 180 : 0
                        Behavior on rotation { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }
                    }
                    HoverHandler { id: pickHover; cursorShape: Qt.PointingHandCursor }
                    TapHandler { onTapped: if (pg.cur && pg.cur.configured) { pop.opened ? pop.close() : pop.open() } }

                    Popup {
                        id: pop
                        objectName: "modelPopup"
                        parent: picker
                        y: picker.height + 4
                        width: picker.width
                        height: 400
                        padding: 0
                        modal: false
                        focus: true
                        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
                        enter: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast } }
                        background: Rectangle { radius: 10; color: Theme.window; border.color: Theme.border }
                        onOpened: { search.text = ""; search.forceActiveFocus(); pg.loadModels(false); pg.runSearch() }
                        contentItem: ColumnLayout {
                            id: panel
                            spacing: 0
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.margins: 10
                                spacing: 8
                                TextField {
                                    id: search
                                    objectName: "modelSearch"
                                    Layout.fillWidth: true
                                    placeholderText: qsTr("Search models")
                                    placeholderTextColor: Theme.textMuted
                                    color: Theme.text
                                    font.pixelSize: 14
                                    selectByMouse: true
                                    onTextChanged: if (pop.opened) pg.runSearch()
                                    Keys.onDownPressed: list.currentIndex = Math.min(list.count - 1, list.currentIndex + 1)
                                    Keys.onUpPressed: list.currentIndex = Math.max(0, list.currentIndex - 1)
                                    onAccepted: if (list.currentIndex >= 0) pg.pickModel(pg.modelRows[list.currentIndex].id)
                                    background: Rectangle {
                                        radius: 8
                                        color: Theme.surface
                                        border.color: search.activeFocus ? Theme.accent : Theme.border
                                        border.width: search.activeFocus ? 1.5 : 1
                                    }
                                }
                                IconButton {
                                    icon: "update"
                                    tip: qsTr("Refresh the list")
                                    enabled: pg.modelsState !== "loading"
                                    onClicked: pg.loadModels(true)
                                }
                            }
                            Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.border }
                            // loading / error banner: the list below still offers the saved model and "Use <typed>"
                            Rectangle {
                                objectName: "modelBanner"
                                visible: pg.modelsState === "loading" || pg.modelsState === "error"
                                Layout.fillWidth: true
                                implicitHeight: bannerRow.implicitHeight + 20
                                color: pg.modelsState === "error" ? Qt.rgba(1, 0.3, 0.3, 0.10) : Theme.surface
                                RowLayout {
                                    id: bannerRow
                                    x: 14; y: 10
                                    width: parent.width - 28
                                    spacing: 10
                                    Spinner { visible: pg.modelsState === "loading" }
                                    Text {
                                        Layout.fillWidth: true
                                        wrapMode: Text.Wrap
                                        font.pixelSize: 12
                                        color: pg.modelsState === "error" ? Theme.danger : Theme.textSubtle
                                        text: pg.modelsState === "error" ? pg.modelsError : qsTr("Loading models from %1…").arg(pg.providerName)
                                    }
                                    AiButton { visible: pg.modelsState === "error"; text: qsTr("Try again"); onClicked: pg.loadModels(true) }
                                }
                            }
                            Item {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                ListView {
                                    id: list
                                    anchors.fill: parent
                                    clip: true
                                    model: pg.modelRows
                                    visible: pg.modelRows.length > 0
                                    boundsBehavior: Flickable.StopAtBounds
                                    ScrollBar.vertical: ScrollBar {}
                                    delegate: Rectangle {
                                        id: row
                                        required property var modelData
                                        required property int index
                                        readonly property bool use: modelData.use === true
                                        width: ListView.view.width
                                        height: 48
                                        color: ListView.isCurrentItem ? Theme.hover : "transparent"
                                        Column {
                                            anchors.left: parent.left; anchors.leftMargin: 14
                                            anchors.right: tags.left; anchors.rightMargin: 10
                                            anchors.verticalCenter: parent.verticalCenter
                                            spacing: 1
                                            Text {
                                                width: parent.width
                                                elide: Text.ElideRight
                                                textFormat: Text.StyledText
                                                font.pixelSize: 14
                                                color: row.modelData.current ? Theme.accent : Theme.text
                                                text: row.use ? qsTr("Use “%1”").arg(pg.esc(row.modelData.id)) : pg.hl(row.modelData.name, row.modelData.nameHl)
                                            }
                                            Text {
                                                width: parent.width
                                                elide: Text.ElideRight
                                                textFormat: Text.StyledText
                                                font.pixelSize: 11
                                                color: Theme.textMuted
                                                visible: row.use || row.modelData.name !== row.modelData.id || !row.modelData.listed
                                                text: row.use ? qsTr("Not in the list. Use this id exactly as typed.")
                                                    : row.modelData.listed ? pg.hl(row.modelData.id, row.modelData.idHl)
                                                    : qsTr("%1 · the provider no longer lists it").arg(pg.esc(row.modelData.id))
                                            }
                                        }
                                        Row {
                                            id: tags
                                            anchors.right: parent.right; anchors.rightMargin: 14
                                            anchors.verticalCenter: parent.verticalCenter
                                            spacing: 8
                                            Rectangle {
                                                visible: !row.use && row.modelData.thinking.length > 0
                                                anchors.verticalCenter: parent.verticalCenter
                                                width: thinkLabel.implicitWidth + 12
                                                height: 18
                                                radius: 9
                                                color: "transparent"
                                                border.color: Theme.border
                                                Text { id: thinkLabel; anchors.centerIn: parent; text: qsTr("thinks"); font.pixelSize: 11; color: Theme.textSubtle }
                                            }
                                            Text {
                                                visible: !row.use && row.modelData.context !== ""
                                                anchors.verticalCenter: parent.verticalCenter
                                                text: qsTr("%1 ctx").arg(row.modelData.context)
                                                font.pixelSize: 11
                                                color: Theme.textMuted
                                            }
                                            IconImage {
                                                visible: row.modelData.current === true
                                                anchors.verticalCenter: parent.verticalCenter
                                                width: 16; height: 16
                                                sourceSize: Qt.size(32, 32)
                                                source: "qrc:/qt/qml/org/rustyinbox/icons/done.svg"
                                                color: Theme.accent
                                            }
                                        }
                                        HoverHandler { cursorShape: Qt.PointingHandCursor; onHoveredChanged: if (hovered) list.currentIndex = row.index }
                                        TapHandler { onTapped: pg.pickModel(row.modelData.id) }
                                    }
                                }
                                Text {
                                    anchors.centerIn: parent
                                    visible: pg.modelsState === "ready" && pg.modelRows.length === 0
                                    text: pg.rowsMeta.total === 0 ? qsTr("This provider lists no models. Type an id to use it.") : qsTr("No model matches.")
                                    font.pixelSize: 13
                                    color: Theme.textMuted
                                }
                            }
                            Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Theme.border; visible: pg.rowsMeta.total > 0 }
                            Text {
                                visible: pg.rowsMeta.total > 0
                                Layout.fillWidth: true
                                Layout.margins: 8
                                font.pixelSize: 11
                                color: Theme.textMuted
                                text: pg.rowsMeta.shown < pg.rowsMeta.total && search.text.trim() === ""
                                    ? qsTr("%1 models. Type to narrow the list.").arg(pg.rowsMeta.total)
                                    : qsTr("%1 of %2 models").arg(pg.rowsMeta.shown).arg(pg.rowsMeta.total)
                            }
                        }
                    }
                }
                IconButton {
                    icon: "update"
                    tip: qsTr("Refresh the model list")
                    enabled: !!pg.cur && pg.cur.configured && pg.modelsState !== "loading"
                    onClicked: pg.loadModels(true)
                }
            }
            // scripted check only: the picker panel shown in the page (grabToImage cannot capture popups)
            Rectangle {
                id: inlineHost
                visible: false
                Layout.fillWidth: true
                Layout.preferredHeight: 400
                radius: 10
                color: Theme.window
                border.color: Theme.border
            }
            RowLayout {
                visible: pg.modelsState === "loading" || pg.modelsState === "error" || (!!pg.cur && !pg.cur.configured)
                spacing: 8
                Spinner { visible: pg.modelsState === "loading" }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    font.pixelSize: 12
                    color: pg.modelsState === "error" ? Theme.danger : Theme.textMuted
                    text: pg.cur && !pg.cur.configured ? qsTr("Connect %1 above to load its models.").arg(pg.providerName)
                        : pg.modelsState === "loading" ? qsTr("Loading models from %1…").arg(pg.providerName)
                        : pg.modelsError
                }
            }
        }
    }

    // ---- thinking
    Rectangle {
        id: thinkCard
        objectName: "thinkingCard"
        visible: pg.provider !== "" && pg.st.modelThinking.length > 0
        Layout.fillWidth: true
        implicitHeight: thinkCol.implicitHeight + 28
        radius: 10
        color: Theme.surface
        ColumnLayout {
            id: thinkCol
            x: 16; y: 14
            width: parent.width - 32
            spacing: 8
            Text { text: qsTr("Thinking"); font.pixelSize: 14; color: Theme.text }
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.pixelSize: 12
                color: Theme.textMuted
                text: qsTr("How much the model reasons before it answers. More is slower and can cost more; Default leaves it to the provider%1.")
                    .arg(pg.st.modelDefaultThinking ? qsTr(" (this model's own default is %1)").arg(pg.levelNames[pg.st.modelDefaultThinking] || pg.st.modelDefaultThinking) : "")
            }
            AiChoice {
                id: thinkChoice
                current: pg.st.thinking || ""
                values: [""].concat(pg.st.modelThinking)
                labels: [qsTr("Default")].concat(pg.st.modelThinking.map(l => pg.levelNames[l] || l))
                onPicked: (v) => pg.act(eng.choose_thinking(v))
            }
        }
    }

    // ---- test connection
    Rectangle {
        id: testCard
        objectName: "testCard"
        visible: pg.provider !== ""
        Layout.fillWidth: true
        implicitHeight: testCol.implicitHeight + 28
        radius: 10
        color: Theme.surface
        ColumnLayout {
            id: testCol
            x: 16; y: 14
            width: parent.width - 32
            spacing: 10
            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                Column {
                    Layout.fillWidth: true
                    spacing: 3
                    Text { text: qsTr("Test connection"); font.pixelSize: 14; color: Theme.text }
                    Text {
                        width: parent.width
                        wrapMode: Text.Wrap
                        font.pixelSize: 12
                        color: Theme.textMuted
                        text: qsTr("Sends one short message (“Reply with the single word OK”) to the chosen model.")
                    }
                }
                AiButton {
                    objectName: "testButton"
                    text: qsTr("Test connection")
                    primary: true
                    enabled: pg.ready && pg.testState !== "busy"
                    onClicked: { pg.testState = "busy"; eng.test_connection() }
                }
            }
            RowLayout {
                visible: pg.testState === "busy"
                spacing: 8
                Spinner {}
                Text { text: qsTr("Asking %1…").arg(pg.st.modelName || pg.st.model || ""); font.pixelSize: 13; color: Theme.textSubtle }
            }
            Rectangle {
                objectName: "testResult"
                visible: pg.testState === "ok" || pg.testState === "error"
                Layout.fillWidth: true
                implicitHeight: resultText.implicitHeight + 20
                radius: 8
                color: pg.testState === "ok" ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12) : Qt.rgba(1, 0.3, 0.3, 0.10)
                border.color: pg.testState === "ok" ? "transparent" : Theme.danger
                Text {
                    id: resultText
                    x: 12; y: 10
                    width: parent.width - 24
                    wrapMode: Text.Wrap
                    font.pixelSize: 13
                    color: pg.testState === "ok" ? Theme.text : Theme.danger
                    text: pg.testState === "ok"
                        ? qsTr("It works. %1 answered “%2” in %3.").arg(pg.st.modelName || pg.st.model).arg(pg.testReply).arg(pg.fmtSeconds(pg.testSeconds))
                        : pg.testError
                }
            }
        }
    }

    // ---- language
    Group { id: grpLanguage; text: qsTr("Answer language") }
    AiOption {
        label: qsTr("Language of the results")
        help: qsTr("Summaries, replies, drafts and translations. “Same as the email” follows the language of the message.")
        AiChoice { key: "aiLanguage"; values: ["auto", "en", "ar"]; labels: [qsTr("Same as the email"), qsTr("English"), qsTr("Arabic")] }
    }

    // ---- features
    Group { id: grpWriting; text: qsTr("Writing") }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Writing tools")
        help: qsTr("Proofread, rewrite, translate, write for me and subject ideas in the composer. Each runs when you click it and shows a preview first.")
        note: qsTr("Sends the draft (and the thread you reply to) to %1 when you use a tool.").arg(pg.providerName)
        AiToggle { key: "aiComposeTools" }
    }
    Group { text: qsTr("Reading") }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Reading tools")
        help: qsTr("Summary, action items, translate, smart replies, attachment summary and phishing check in the reader. Each runs when you click it.")
        note: qsTr("Sends the conversation to %1 when you use a tool.").arg(pg.providerName)
        AiToggle { key: "aiReadTools" }
    }
    AiOption {
        enabled: pg.featuresOn && pg.host.prefs.aiReadTools === true
        label: qsTr("Summarize long conversations automatically")
        help: qsTr("Shows the summary when you open a long conversation, without a click.")
        note: qsTr("Automatic: sends the conversation to %1 as soon as you open it.").arg(pg.providerName)
        AiToggle { key: "aiAutoSummary" }
    }
    AiOption {
        enabled: pg.featuresOn && pg.host.prefs.aiReadTools === true
        label: qsTr("Suggest replies automatically")
        help: qsTr("Shows three short replies under a message when you open it.")
        note: qsTr("Automatic: sends the conversation to %1 as soon as you open it.").arg(pg.providerName)
        AiToggle { key: "aiAutoReplies" }
    }
    AiOption {
        enabled: pg.featuresOn && pg.host.prefs.aiReadTools === true
        label: qsTr("Check mail from new senders for phishing automatically")
        help: qsTr("Marks suspicious messages from people you have not written to before.")
        note: qsTr("Automatic: sends the sender details and the message to %1 when you open it.").arg(pg.providerName)
        AiToggle { key: "aiAutoPhishing" }
    }
    Group { id: grpOrganize; text: qsTr("Organizing in the background") }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Smart filtering")
        help: qsTr("New mail the rules are unsure about is sorted into People, Notifications, Newsletters or Invitations. It never moves mail on the server.")
        note: qsTr("Background: sends the sender, subject and the start of every such new message to %1.").arg(pg.providerName)
        AiToggle { key: "aiSmartFilter" }
    }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Needs reply")
        help: qsTr("Marks new mail that expects an answer from you and lists it in its own view.")
        note: qsTr("Background: sends the sender, subject and the start of every new message to %1.").arg(pg.providerName)
        AiToggle { key: "aiNeedsReply" }
    }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Follow-up reminders")
        help: qsTr("Mail you sent that asked for something and got no answer shows in a Follow up list, with one notification.")
        note: qsTr("Background: sends the mail you sent, and replies to it, to %1.").arg(pg.providerName)
        AiToggle { key: "aiFollowUps" }
    }
    AiOption {
        enabled: pg.featuresOn && pg.host.prefs.aiFollowUps === true
        label: qsTr("Remind me after")
        AiChoice { key: "aiFollowUpDays"; values: [2, 3, 5, 7, 14]; labels: [qsTr("2 days"), qsTr("3 days"), qsTr("5 days"), qsTr("7 days"), qsTr("14 days")] }
    }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Ask in plain words")
        help: qsTr("Type a question in the search box (“invoices from Zenit last month”) and it becomes a search you can edit.")
        note: qsTr("Sends only the question you type to %1.").arg(pg.providerName)
        AiToggle { key: "aiSearch" }
    }
    AiOption {
        enabled: pg.featuresOn
        label: qsTr("Daily briefing")
        help: qsTr("Once a day, a digest of your unread mail with people first and what needs attention. You can also ask for it any time.")
        note: qsTr("Background: sends the sender, subject and the start of your unread messages to %1.").arg(pg.providerName)
        AiToggle { key: "aiBriefing" }
    }
    AiOption {
        enabled: pg.featuresOn && pg.host.prefs.aiBriefing === true
        label: qsTr("Briefing time")
        AiChoice {
            key: "aiBriefingHour"
            values: [5, 6, 7, 8, 9, 10, 11, 12]
            labels: ["5:00", "6:00", "7:00", "8:00", "9:00", "10:00", "11:00", "12:00"]
        }
    }

    // ---- excluded accounts
    Group { id: grpExcluded; text: qsTr("Accounts excluded from AI") }
    Text {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        font.pixelSize: 13
        color: Theme.textSubtle
        text: qsTr("Nothing from a ticked account is ever sent to an AI provider, by any feature, even when you click a tool.")
    }
    Text {
        visible: pg.host.accounts.length === 0
        text: qsTr("No accounts yet.")
        color: Theme.textMuted
        font.pixelSize: 13
    }
    Repeater {
        model: pg.host.accounts
        delegate: Rectangle {
            id: acc
            required property var modelData
            Layout.fillWidth: true
            implicitHeight: 52
            radius: 10
            color: Theme.surface
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 16; anchors.rightMargin: 16
                spacing: 12
                TickBox {
                    checked: pg.isExcluded(acc.modelData.email)
                    onToggled: pg.toggleExcluded(acc.modelData.email)
                }
                Column {
                    Layout.fillWidth: true
                    spacing: 1
                    Text { text: acc.modelData.name || acc.modelData.email; font.pixelSize: 14; color: Theme.text }
                    Text { visible: !!acc.modelData.name; text: acc.modelData.email; font.pixelSize: 12; color: Theme.textMuted }
                }
                Text {
                    visible: pg.isExcluded(acc.modelData.email)
                    text: qsTr("Excluded")
                    font.pixelSize: 12
                    color: Theme.textSubtle
                }
            }
            HoverHandler { cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: pg.toggleExcluded(acc.modelData.email) }
        }
    }

    // ---- privacy
    Group { id: grpPrivacy; text: qsTr("Privacy") }
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: privacy.implicitHeight + 28
        radius: 10
        color: Theme.surface
        Text {
            id: privacy
            x: 16; y: 14
            width: parent.width - 32
            wrapMode: Text.Wrap
            font.pixelSize: 12
            lineHeight: 1.25
            color: Theme.textSubtle
            text: qsTr("Nothing is sent until you turn AI on and choose a provider.\nMail goes only to the provider you choose (%1), never to the developers of this app.\nAPI keys and the ChatGPT sign-in stay in this computer's secret store, and only the last 4 characters of a key are ever shown.\nTools you click send only what they need. Switches marked Background or Automatic send mail content on their own, so they are off until you turn them on.\nAnswers can be wrong. Check them before you rely on them.").arg(pg.providerName)
        }
    }
}
