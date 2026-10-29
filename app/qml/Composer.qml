import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtWebEngine

// Composer for new mail, replies and forwards. The body is an HTML editor (contenteditable in a
// web view) with a formatting toolbar. Drafts save themselves every few seconds and go to the
// server's Drafts folder when the composer closes. Sending goes through the outbox (undo send /
// send later), so `sendRequested` only hands the draft over.
Rectangle {
    id: root
    property var backend
    property var accounts: []           // sidebar accounts [{email, name}]
    property var draft: ({})
    property int outboxId: 0
    property bool expanded: false
    property bool dirty: false
    property bool editorReady: false
    property string pendingHtml: ""
    property var fmt: ({})              // active formats under the caret
    property bool showCc: false
    property bool showBcc: false
    property bool aiOn: false           // AI: the writing assistant button shows
    property alias tools: aiTools
    signal closed()
    // web view calls with a result callback still on the way. The view must not be destroyed while
    // one is pending: Qt WebEngine delivers the result to the dead view and crashes (Qt 6.12).
    property int jsPending: 0
    property bool closing: false
    property var idleFns: []
    function js(code, cb) {
        jsPending++
        web.runJavaScript(code, function(r) {
            root.jsPending--
            if (!root.closing) cb(r)          // a late autosave must not save a draft that was just discarded
            if (root.jsPending === 0 && root.closing) { const f = root.idleFns; root.idleFns = []; f.forEach(fn => Qt.callLater(fn)) }
        })
    }
    // run fn once no web view call is pending; nothing else runs on this composer after that
    function whenIdle(fn) {
        closing = true
        if (jsPending === 0) Qt.callLater(fn)
        else idleFns = idleFns.concat([fn])
    }
    signal sendRequested(var draft, int outboxId, double sendAt)
    signal notice(string text)
    signal toggleExpand()

    color: Theme.window
    focus: true

    readonly property string title: ({ reply: qsTr("Reply"), replyAll: qsTr("Reply all"), forward: qsTr("Forward"), draft: qsTr("Draft") })[draft.kind] || qsTr("New message")

    function load(d, id) {
        draft = d
        outboxId = id || 0
        toField.addresses = d.to || []
        ccField.addresses = d.cc || []
        bccField.addresses = d.bcc || []
        showCc = (d.cc || []).length > 0
        showBcc = (d.bcc || []).length > 0
        subject.text = d.subject || ""
        attachModel.clear()
        for (const a of (d.attachments || [])) attachModel.append({ name: a.name, mime: a.mime || "", size: a.size || 0, path: a.path || "", fromAttachment: a.fromAttachment || 0 })
        pendingHtml = d.html || "<p><br></p>"
        dirty = false
        refreshAi()
        if (editorReady) setEditorHtml(pendingHtml)
        Qt.callLater(() => {
            if ((d.to || []).length === 0) toField.focusInput()
            else focusEditor()
        })
    }

    // scripted checks (--autotest)
    function templateForTest(name) { saveAsTemplate(name) }
    function fillForTest(to, subj, html) {
        toField.addresses = to
        subject.text = subj
        setEditorHtml(html)
        dirty = true
    }

    // ---- AI writing assistant: operates on the user's own text (above the signature and the quote)
    function refreshAi() { aiOn = aiTools.enabled() }
    Connections { target: root.backend; function onSettings_changed(json) { root.refreshAi() } }
    function ownText(cb) {
        root.js("ownText()", function(t) {
            try { cb(String(t || "")) } catch (e) { console.warn("composer: ai text failed " + e) }
        })
    }
    function paragraphsHtml(text) {
        const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
        return String(text).trim().split(/\n{2,}/).map(p => "<p dir=\"auto\">" + esc(p).replace(/\n/g, "<br>") + "</p>").join("")
    }
    function applyOwn(text, mode) {
        web.runJavaScript("try{applyOwn(" + JSON.stringify(paragraphsHtml(text)) + "," + JSON.stringify(mode) + ")}catch(e){console.warn('applyOwn '+e)}")
        dirty = true
    }
    // a smart reply chosen in the reader: it goes above the signature of the new reply
    function prefillReply(text) {
        pendingHtml = String(pendingHtml).replace("<p><br></p>", paragraphsHtml(text))
        if (editorReady) setEditorHtml(pendingHtml)
        dirty = true
    }

    function setEditorHtml(html) {
        web.runJavaScript("setContent(" + JSON.stringify(html) + ")")
    }
    function focusEditor() { web.forceActiveFocus(); if (editorReady) web.runJavaScript("focusStart()") }
    function exec(cmd, arg) {
        web.runJavaScript("document.execCommand(" + JSON.stringify(cmd) + ",false," + JSON.stringify(arg || null) + ");ed.focus();report()")
        dirty = true
    }

    // the current draft, with the editor's html (async: the web view answers in a callback)
    function collect(then) {
        toField.finish(); ccField.finish(); bccField.finish()
        root.js("document.getElementById('ed') ? document.getElementById('ed').innerHTML : ''", function(html) {
            try { then(root.snapshot(html)) } catch (e) { console.warn("composer: failed " + e) }
        })
    }
    function snapshot(html) {
        const atts = []
        for (let i = 0; i < attachModel.count; i++) {
            const a = attachModel.get(i)
            const o = { name: a.name, mime: a.mime, size: a.size }
            if (a.path) o.path = a.path
            if (a.fromAttachment) o.fromAttachment = a.fromAttachment
            atts.push(o)
        }
        const acc = root.accounts.find(x => x.email === root.draft.account)
        return Object.assign({}, root.draft, {
            to: toField.addresses, cc: ccField.addresses, bcc: bccField.addresses,
            subject: subject.text, html: html || "", attachments: atts,
            fromName: acc && acc.name ? acc.name : (root.draft.fromName || "")
        })
    }
    function isEmpty(d) {
        const text = String(d.html || "").replace(/<div class="ri-signature">[\s\S]*$/, "").replace(/<[^>]*>/g, "").replace(/&nbsp;/g, " ").trim()
        return !text && !(d.subject || "").trim() && (d.to || []).length === 0 && (d.cc || []).length === 0
            && (d.attachments || []).length === 0 && (d.kind === "new" || !d.kind)
    }
    function save(upload, then) {
        collect(function(d) {
            if (root.isEmpty(d) && root.outboxId === 0) { if (then) then(false); return }
            root.outboxId = root.backend.save_draft(root.outboxId, JSON.stringify(d), upload)
            root.dirty = false
            if (then) then(true)
        })
    }
    function closeComposer() {
        save(true, function(saved) {
            if (saved) root.notice(qsTr("Draft saved"))
            root.closed()
        })
    }
    function discard() {
        collect(function(d) {
            root.backend.discard_draft(root.outboxId, JSON.stringify(d))
            root.notice(qsTr("Draft discarded"))
            root.closed()
        })
    }
    function send(sendAt) {
        collect(function(d) {
            if ((d.to.length + d.cc.length + d.bcc.length) === 0) { root.notice(qsTr("Add at least one recipient")); toField.focusInput(); return }
            const bad = d.to.concat(d.cc, d.bcc).find(a => !toField.valid(a.address))
            if (bad) { root.notice(qsTr("\"%1\" is not a valid email address").arg(bad.address)); return }
            if (!d.subject.trim() && sendAt === 0 && !noSubject.confirmed) { noSubject.open(); return }
            noSubject.confirmed = false
            root.outboxId = root.backend.save_draft(root.outboxId, JSON.stringify(d), false)
            root.sendRequested(d, root.outboxId, sendAt)
        })
    }
    function addFile(url) {
        const info = JSON.parse(backend.file_info_json(String(url)))
        if (!info) return
        for (let i = 0; i < attachModel.count; i++) if (attachModel.get(i).path === info.path) return
        attachModel.append({ name: info.name, mime: info.mime, size: info.size, path: info.path, fromAttachment: 0 })
        dirty = true
    }
    function sizeLabel(n) {
        if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB"
        if (n >= 1024) return Math.round(n / 1024) + " KB"
        return n + " B"
    }

    Shortcut { sequences: ["Ctrl+Return", "Ctrl+Enter"]; enabled: root.visible; onActivated: root.send(0) }
    Shortcut { sequence: "Escape"; enabled: root.visible; onActivated: aiTools.active ? aiTools.dismiss() : root.closeComposer() }
    Shortcut { sequence: "Ctrl+J"; enabled: root.visible && root.aiOn; onActivated: aiTools.openMenu() }
    Shortcut { sequence: "Ctrl+Shift+A"; enabled: root.visible; onActivated: fileDialog.open() }

    Timer { interval: 4000; repeat: true; running: root.visible && root.dirty && !root.closing; onTriggered: root.save(false) }

    ListModel { id: attachModel }

    // ---- templates
    property var templates: []
    function insertTemplate(t) {
        if (!subject.text.trim() && t.subject) subject.text = t.subject
        web.runJavaScript("insertAtCaret(" + JSON.stringify(t.html) + ")")
        dirty = true
    }
    function saveAsTemplate(name) {
        collect(function(d) {
            const html = String(d.html).replace(/<div class="ri-signature">[\s\S]*?<\/div>/, "").replace(/<blockquote class="ri-quote"[\s\S]*$/, "")
            const r = JSON.parse(root.backend.save_template(JSON.stringify({ name: name, subject: d.subject, html: html })))
            root.notice(r.error ? r.error : qsTr("Template \"%1\" saved").arg(name))
        })
    }
    Menu {
        id: tplMenu
        Instantiator {
            model: root.templates
            delegate: MenuItem {
                required property var modelData
                text: modelData.name
                onTriggered: root.insertTemplate(modelData)
            }
            onObjectAdded: (index, object) => tplMenu.insertItem(index, object)
            onObjectRemoved: (index, object) => tplMenu.removeItem(object)
        }
        MenuItem { enabled: false; visible: root.templates.length === 0; height: visible ? implicitHeight : 0; text: qsTr("No templates yet") }
        MenuSeparator {}
        MenuItem { text: qsTr("Save this message as a template…"); onTriggered: tplDialog.open() }
    }
    Dialog {
        id: tplDialog
        anchors.centerIn: parent
        modal: true
        title: qsTr("Save as template")
        standardButtons: Dialog.Cancel | Dialog.Save
        onOpened: { tplName.text = subject.text; tplName.forceActiveFocus(); tplName.selectAll() }
        onAccepted: root.saveAsTemplate(tplName.text.trim())
        TextField { id: tplName; width: 320; placeholderText: qsTr("Template name"); onAccepted: tplDialog.accept() }
    }
    FileDialog {
        id: fileDialog
        title: qsTr("Attach files")
        fileMode: FileDialog.OpenFiles
        onAccepted: { for (const f of selectedFiles) root.addFile(f) }
    }
    Dialog {
        id: noSubject
        property bool confirmed: false
        anchors.centerIn: parent
        modal: true
        title: qsTr("Send without a subject?")
        standardButtons: Dialog.Cancel | Dialog.Ok
        onAccepted: { confirmed = true; root.send(0) }
    }
    SnoozeMenu {
        id: laterMenu
        onPicked: (untilMs, label) => root.send(untilMs)
    }
    Popup {
        id: linkPopup
        x: linkBtn.x + toolbar.x
        y: toolbar.y + toolbar.height
        padding: 10
        background: Rectangle { radius: 10; color: Theme.surface; border.color: Theme.border }
        onOpened: linkField.forceActiveFocus()
        Row {
            spacing: 8
            TextField {
                id: linkField
                width: 280
                placeholderText: "https://"
                color: Theme.text
                onAccepted: { root.exec("createLink", text.match(/^[a-z]+:/i) ? text : "https://" + text); text = ""; linkPopup.close() }
            }
        }
    }

    // ---- header
    Rectangle {
        id: head
        width: parent.width
        height: 48
        color: Theme.window
        Text {
            x: 18
            anchors.verticalCenter: parent.verticalCenter
            text: root.title
            font.pixelSize: 16
            font.weight: Font.DemiBold
            color: Theme.textStrong
        }
        Row {
            anchors.right: parent.right
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            IconButton { icon: root.expanded ? "collapse" : "expand"; tip: root.expanded ? qsTr("Restore") : qsTr("Expand"); onClicked: root.toggleExpand() }
            IconButton { icon: "close"; tip: qsTr("Close and keep the draft (Esc)"); onClicked: root.closeComposer() }
        }
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
    }

    // ---- fields
    Column {
        id: fields
        anchors.top: head.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 18
        anchors.rightMargin: 18
        spacing: 0

        Item {
            visible: root.accounts.length > 1
            width: parent.width
            height: 44
            Text { y: 13; text: qsTr("From"); font.pixelSize: 13; color: Theme.textMuted }
            ComboBox {
                id: fromBox
                x: 44
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(420, parent.width - 44)
                model: root.accounts.map(a => (a.name ? a.name + "  <" + a.email + ">" : a.email))
                currentIndex: Math.max(0, root.accounts.findIndex(a => a.email === root.draft.account))
                onActivated: (i) => {
                    const a = root.accounts[i]
                    root.draft = Object.assign({}, root.draft, { account: a.email, fromName: a.name || "" })
                    root.dirty = true
                }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
        }
        Item {
            width: parent.width
            height: toField.height
            RecipientField {
                id: toField
                width: parent.width - ccLinks.width - 8
                label: qsTr("To")
                backend: root.backend
                onEdited: root.dirty = true
            }
            Row {
                id: ccLinks
                anchors.right: parent.right
                y: 12
                spacing: 12
                Text {
                    visible: !root.showCc
                    text: qsTr("Cc"); color: Theme.accent; font.pixelSize: 13
                    HoverHandler { cursorShape: Qt.PointingHandCursor }
                    TapHandler { onTapped: { root.showCc = true; Qt.callLater(ccField.focusInput) } }
                }
                Text {
                    visible: !root.showBcc
                    text: qsTr("Bcc"); color: Theme.accent; font.pixelSize: 13
                    HoverHandler { cursorShape: Qt.PointingHandCursor }
                    TapHandler { onTapped: { root.showBcc = true; Qt.callLater(bccField.focusInput) } }
                }
            }
        }
        RecipientField { id: ccField; visible: root.showCc; width: parent.width; label: qsTr("Cc"); backend: root.backend; onEdited: root.dirty = true }
        RecipientField { id: bccField; visible: root.showBcc; width: parent.width; label: qsTr("Bcc"); backend: root.backend; onEdited: root.dirty = true }
        Item {
            width: parent.width
            height: 46
            TextField {
                id: subject
                anchors.fill: parent
                leftPadding: 0
                placeholderText: qsTr("Subject")
                font.pixelSize: 16
                font.weight: Font.DemiBold
                color: Theme.textStrong
                background: Item {}
                onTextEdited: root.dirty = true
                Keys.onReturnPressed: root.focusEditor()
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
        }
        AiSubjectChips {
            model: aiTools.subjects
            onPicked: (s) => { subject.text = s; root.dirty = true; aiTools.subjects = [] }
            onDismissed: aiTools.subjects = []
        }
    }

    // ---- formatting toolbar
    Row {
        id: toolbar
        anchors.top: fields.bottom
        anchors.topMargin: 6
        x: 12
        spacing: 2
        component Fmt: IconButton { size: 30 }
        component Letter: Rectangle {
            id: lb
            property string glyph
            property string cmd
            property string tip
            property bool on: root.fmt[cmd] === true
            width: 30; height: 30; radius: 7
            color: on ? Theme.selected : (lh.hovered ? Theme.hover : "transparent")
            Text {
                anchors.centerIn: parent
                text: lb.glyph
                font.pixelSize: 15
                font.bold: lb.cmd === "bold"
                font.italic: lb.cmd === "italic"
                font.underline: lb.cmd === "underline"
                font.strikeout: lb.cmd === "strikeThrough"
                color: lb.on ? Theme.accent : Theme.icon
            }
            HoverHandler { id: lh; cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: root.exec(lb.cmd) }
            ToolTip.visible: lh.hovered
            ToolTip.delay: 600
            ToolTip.text: lb.tip
        }
        Letter { glyph: "B"; cmd: "bold"; tip: qsTr("Bold (Ctrl+B)") }
        Letter { glyph: "I"; cmd: "italic"; tip: qsTr("Italic (Ctrl+I)") }
        Letter { glyph: "U"; cmd: "underline"; tip: qsTr("Underline (Ctrl+U)") }
        Letter { glyph: "S"; cmd: "strikeThrough"; tip: qsTr("Strikethrough") }
        Rectangle { width: 1; height: 20; color: Theme.border; anchors.verticalCenter: parent.verticalCenter }
        Fmt { icon: "list-bullet"; tip: qsTr("Bulleted list"); active: root.fmt.insertUnorderedList === true; onClicked: root.exec("insertUnorderedList") }
        Fmt { icon: "list-number"; tip: qsTr("Numbered list"); active: root.fmt.insertOrderedList === true; onClicked: root.exec("insertOrderedList") }
        Fmt { icon: "quote"; tip: qsTr("Quote"); onClicked: root.exec("formatBlock", "blockquote") }
        Fmt { id: linkBtn; icon: "link"; tip: qsTr("Link"); onClicked: linkPopup.open() }
        Fmt { icon: "clear-format"; tip: qsTr("Clear formatting"); onClicked: { root.exec("removeFormat"); root.exec("formatBlock", "p") } }
        Rectangle { visible: root.aiOn; width: 1; height: 20; color: Theme.border; anchors.verticalCenter: parent.verticalCenter }
        Fmt { id: aiBtn; objectName: "aiButton"; visible: root.aiOn; icon: "sparkle"; tip: qsTr("Writing assistant (Ctrl+J)"); active: aiTools.active; onClicked: aiTools.openMenu() }
    }

    // ---- body editor
    Rectangle {
        id: editorBox
        anchors.top: toolbar.bottom
        anchors.topMargin: 4
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: attachFlow.top
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        color: "transparent"
        WebEngineView {
            id: web
            anchors.fill: parent
            backgroundColor: Theme.window
            settings.javascriptCanOpenWindows: false
            settings.pluginsEnabled: false
            settings.localContentCanAccessRemoteUrls: true
            Component.onCompleted: loadHtml(root.editorPage(), "about:blank")
            onLoadingChanged: (req) => {
                if (req.status === WebEngineView.LoadSucceededStatus) {
                    root.editorReady = true
                    root.setEditorHtml(root.pendingHtml)
                }
            }
            // the page reports through its title: "dirty:n", "fmt:<flags>", "send:n"
            onTitleChanged: {
                const t = String(title)
                if (t.startsWith("dirty:")) root.dirty = true
                else if (t.startsWith("send:")) root.send(0)
                else if (t.startsWith("fmt:")) {
                    const f = t.substring(4)
                    const keys = ["bold", "italic", "underline", "strikeThrough", "insertUnorderedList", "insertOrderedList"]
                    const o = {}
                    keys.forEach((k, i) => o[k] = f.charAt(i) === "1")
                    root.fmt = o
                }
            }
            onJavaScriptConsoleMessage: (level, message, line, source) => console.warn("composer js:", message, line)
            onNavigationRequested: (request) => {
                if (request.navigationType === WebEngineNavigationRequest.LinkClickedNavigation) {
                    Qt.openUrlExternally(request.url)
                    request.reject()
                }
            }
        }
    }

    // ---- attachments
    Flow {
        id: attachFlow
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: bottomBar.top
        anchors.leftMargin: 18
        anchors.rightMargin: 18
        anchors.bottomMargin: attachModel.count ? 8 : 0
        spacing: 8
        Repeater {
            model: attachModel
            delegate: Rectangle {
                required property string name
                required property double size
                required property int index
                height: 34
                width: attRow.implicitWidth + 20
                radius: 8
                color: Theme.surface
                border.color: Theme.border
                Row {
                    id: attRow
                    anchors.centerIn: parent
                    spacing: 8
                    IconImage { anchors.verticalCenter: parent.verticalCenter; width: 15; height: 15; sourceSize: Qt.size(30, 30); source: "qrc:/qt/qml/org/rustyinbox/icons/paperclip.svg"; color: Theme.icon }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: name; font.pixelSize: 13; color: Theme.text; elide: Text.ElideMiddle; width: Math.min(implicitWidth, 260) }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: size > 0 ? root.sizeLabel(size) : ""; font.pixelSize: 11; color: Theme.textMuted }
                    IconImage {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 12; height: 12; sourceSize: Qt.size(24, 24)
                        source: "qrc:/qt/qml/org/rustyinbox/icons/close.svg"
                        color: Theme.textMuted
                        HoverHandler { cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: { attachModel.remove(index); root.dirty = true } }
                    }
                }
            }
        }
    }

    // ---- bottom bar: send, send later, attach, discard
    Rectangle {
        id: bottomBar
        anchors.bottom: parent.bottom
        width: parent.width
        height: 60
        color: Theme.window
        Rectangle { width: parent.width; height: 1; color: Theme.border }
        Row {
            x: 16
            anchors.verticalCenter: parent.verticalCenter
            spacing: 10
            Rectangle {
                id: sendBtn
                objectName: "sendButton"
                width: sendRow.implicitWidth + 30
                height: 38
                radius: 9
                color: sh.hovered ? Qt.darker(Theme.accent, 1.08) : Theme.accent
                scale: st.pressed ? 0.96 : 1
                Behavior on scale { NumberAnimation { duration: Theme.fast } }
                Row {
                    id: sendRow
                    anchors.centerIn: parent
                    spacing: 8
                    IconImage { anchors.verticalCenter: parent.verticalCenter; width: 16; height: 16; sourceSize: Qt.size(32, 32); source: "qrc:/qt/qml/org/rustyinbox/icons/send.svg"; color: Theme.accentText }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Send"); font.pixelSize: 14; font.weight: Font.DemiBold; color: Theme.accentText }
                }
                HoverHandler { id: sh; cursorShape: Qt.PointingHandCursor }
                TapHandler { id: st; onTapped: root.send(0) }
                ToolTip.visible: sh.hovered
                ToolTip.delay: 600
                ToolTip.text: qsTr("Send (Ctrl+Enter)")
            }
            IconButton { id: laterBtn; anchors.verticalCenter: parent.verticalCenter; icon: "clock"; tip: qsTr("Send later"); onClicked: laterMenu.popup(laterBtn, 0, -laterMenu.implicitHeight - 6) }
            IconButton { anchors.verticalCenter: parent.verticalCenter; icon: "paperclip"; tip: qsTr("Attach files (Ctrl+Shift+A)"); onClicked: fileDialog.open() }
            IconButton {
                id: tplBtn
                objectName: "templatesButton"
                anchors.verticalCenter: parent.verticalCenter
                icon: "template"
                tip: qsTr("Templates")
                onClicked: { root.templates = JSON.parse(root.backend.templates_json()); tplMenu.popup(tplBtn, 0, -tplMenu.implicitHeight - 6) }
            }
        }
        IconButton {
            anchors.right: parent.right
            anchors.rightMargin: 14
            anchors.verticalCenter: parent.verticalCenter
            icon: "trash"
            tip: qsTr("Discard draft")
            onClicked: root.discard()
        }
    }

    AiComposerTools {
        id: aiTools
        anchors.fill: parent
        z: 10
        composer: root
        menuAnchor: aiBtn
        bottomInset: bottomBar.height + attachFlow.height + (attachModel.count ? 8 : 0) + 8
    }

    // drop files anywhere on the composer
    DropArea {
        anchors.fill: parent
        z: 20
        keys: ["text/uri-list"]
        onDropped: (drop) => { for (const u of drop.urls) root.addFile(u); drop.accept() }
        Rectangle {
            anchors.fill: parent
            anchors.margins: 8
            radius: 14
            visible: parent.containsDrag
            color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.10)
            border.color: Theme.accent
            border.width: 2
            Text { anchors.centerIn: parent; text: qsTr("Drop to attach"); font.pixelSize: 18; font.weight: Font.DemiBold; color: Theme.accent }
        }
    }

    function editorPage() {
        const c = (x) => String(x)
        const font = (Theme.font ? "'" + Theme.font + "'," : "") + "sans-serif"
        return "<!doctype html><html><head><meta charset='utf-8'><style>"
            + "html,body{margin:0;height:100%;background:" + c(Theme.window) + "}"
            + "#ed{min-height:100%;box-sizing:border-box;padding:10px 6px 40px;outline:none;font-family:" + font + ";font-size:15px;line-height:1.55;color:" + c(Theme.text) + ";overflow-wrap:anywhere}"
            + "#ed:empty:before{content:'" + qsTr("Write your message…").replace(/'/g, "\\'") + "';color:" + c(Theme.textMuted) + "}"
            + "#ed p{margin:0 0 .6em}a{color:" + c(Theme.accent) + "}"
            + "blockquote{margin:0 0 0 .8ex;border-left:2px solid " + c(Theme.border) + ";padding-left:1ex;color:" + c(Theme.textSubtle) + "}"
            + ".ri-signature{color:" + c(Theme.textMuted) + "}img{max-width:100%}"
            + "</style></head><body><div id='ed' contenteditable='true'></div><script>"
            + "const ed=document.getElementById('ed');let n=0;"
            + "function setContent(h){ed.innerHTML=h;}"
            + "function insertAtCaret(h){ed.focus();const s=getSelection();if(!s.rangeCount||!ed.contains(s.anchorNode)){const r=document.createRange();r.setStart(ed,0);r.collapse(true);s.removeAllRanges();s.addRange(r);}document.execCommand('insertHTML',false,h);document.title='dirty:'+(n++);}"
            + "function ownMarker(){let m=null;for(const x of [ed.querySelector('.ri-signature'),ed.querySelector('blockquote.ri-quote')]){if(!x)continue;let c=x;while(c&&c.parentNode!==ed)c=c.parentNode;if(c&&(!m||(m.compareDocumentPosition(c)&Node.DOCUMENT_POSITION_PRECEDING)))m=c;}"
            + "if(m){while(m.previousSibling&&((m.previousSibling.nodeType===1&&(m.previousSibling.tagName==='BR'||(m.previousSibling.tagName==='DIV'&&/(wrote:|Forwarded message)/.test(m.previousSibling.textContent))))||(m.previousSibling.nodeType===3&&!m.previousSibling.data.trim())))m=m.previousSibling;}return m;}"
            + "function ownNodes(){const m=ownMarker();const out=[];for(const n of ed.childNodes){if(n===m)break;out.push(n);}return out;}"
            + "function ownText(){let out='';const blk=/^(P|DIV|LI|UL|OL|BLOCKQUOTE|H[1-6]|PRE|TABLE|TR)$/;"
            + "function walk(n){if(n.nodeType===3){out+=n.data.replace(/\\u00a0/g,' ');return}if(n.nodeType!==1)return;if(n.tagName==='BR'){out+='\\n';return}"
            + "const b=blk.test(n.tagName);if(b&&out&&!out.endsWith('\\n'))out+='\\n';n.childNodes.forEach(walk);if(b&&out){if(n.tagName==='P'){if(!out.endsWith('\\n\\n'))out+=out.endsWith('\\n')?'\\n':'\\n\\n'}else if(!out.endsWith('\\n'))out+='\\n'}}"
            + "ownNodes().forEach(walk);return out.replace(/\\n{3,}/g,'\\n\\n').trim();}"
            + "function applyOwn(h,mode){const m=ownMarker();ed.focus();const s=getSelection();const r=document.createRange();"
            + "if(mode==='replace'){r.setStart(ed,0);if(m)r.setEndBefore(m);else r.setEnd(ed,ed.childNodes.length);}else{if(m)r.setStartBefore(m);else r.setStart(ed,ed.childNodes.length);r.collapse(true);}"
            + "s.removeAllRanges();s.addRange(r);document.execCommand('insertHTML',false,h);document.title='dirty:'+(n++);}"
            + "function focusStart(){ed.focus();const r=document.createRange();r.setStart(ed,0);r.collapse(true);const s=getSelection();s.removeAllRanges();s.addRange(r);}"
            + "function report(){const k=['bold','italic','underline','strikeThrough','insertUnorderedList','insertOrderedList'];document.title='fmt:'+k.map(c=>document.queryCommandState(c)?1:0).join('')+':'+(n++);}"
            + "ed.addEventListener('input',()=>{document.title='dirty:'+(n++)});"
            + "document.addEventListener('selectionchange',report);"
            + "document.addEventListener('keydown',e=>{if(e.ctrlKey&&e.key==='Enter'){e.preventDefault();document.title='send:'+(n++)}});"
            + "ed.addEventListener('paste',e=>{const h=e.clipboardData.getData('text/html');if(!h){const t=e.clipboardData.getData('text/plain');if(t){e.preventDefault();document.execCommand('insertText',false,t)}}});"
            + "</script></body></html>"
    }
}
