import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtWebEngine

// Conversation reader: toolbar + the messages. Each message renders in its own sandboxed iframe
// (email scripts never run, email CSS stays isolated); only this page's small script runs, to
// size the frames. Links open in the system browser.
Rectangle {
    id: root
    color: Theme.window

    property var thread: null           // the list row of the open thread
    property string listName: "inbox"
    signal action(string name)
    signal snooze(Item anchor)
    signal close()
    signal respond(string kind)
    signal contact(string address, string name)     // a name or address was clicked
    signal openAttachment(int id)
    signal saveAttachment(int id, string name)
    signal saveAttachments(var ids)
    signal smartReply(string text)                  // AI: a suggested reply was clicked
    property alias assistantPanel: aiPanel          // AI strip + cards (scripted checks reach in here)
    property var translated: null                   // AI: {id, text, target} of the translated newest message
    property bool showTr: false

    // links of this page (people, attachments) go to the app, never to the network
    readonly property string appLink: "https://rusty-inbox.invalid/"

    function esc(s) {
        return String(s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;")
    }
    function addrOf(a) { return typeof a === "string" ? a : (a.mailbox || a.address || a.email || "") }
    function nameOf(a) { return typeof a === "string" ? "" : (a.name || "") }
    // a clickable name (the address shows on hover), opening the person's card
    function personLink(address, name, cls, label) {
        if (!address) return esc(label || name)
        return "<a class='" + cls + "' href='" + esc(appLink + "contact?a=" + encodeURIComponent(address) + "&n=" + encodeURIComponent(name || ""))
            + "' title='" + esc(address) + "'>" + esc(label || name || address) + "</a>"
    }
    function people(list) {
        return (list || []).filter(a => addrOf(a) || nameOf(a)).map(a => personLink(addrOf(a), nameOf(a), "person")).join(", ")
    }
    function sizeLabel(n) {
        if (!(n > 0)) return ""
        if (n < 1024) return n + " B"
        if (n < 1024 * 1024) return Math.round(n / 1024) + " KB"
        return (n / 1024 / 1024).toFixed(n < 10 * 1024 * 1024 ? 1 : 0) + " MB"
    }
    readonly property string clipSvg: "<svg viewBox='0 0 24 24' width='15' height='15' fill='none' stroke='currentColor' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M20 11.5l-7.8 7.8a5 5 0 0 1-7.1-7.1l8.1-8.1a3.4 3.4 0 0 1 4.8 4.8l-8.1 8.1a1.7 1.7 0 0 1-2.4-2.4l7.4-7.4'/></svg>"
    readonly property string downloadSvg: "<svg viewBox='0 0 24 24' width='15' height='15' fill='none' stroke='currentColor' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M12 4v11'/><path d='M7.5 10.5L12 15l4.5-4.5'/><path d='M5 19.5h14'/></svg>"
    readonly property string sparkSvg: "<svg viewBox='0 0 24 24' width='15' height='15' fill='none' stroke='currentColor' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z'/><path d='M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z'/></svg>"
    function attachmentChips(list) {
        if (!list.length) return ""
        const chips = list.map(a => "<span class='att' id='att-" + a.id + "'>"
            + "<a class='open' href='" + appLink + "attachment?do=open&id=" + a.id + "' title='" + esc(qsTr("Open %1").arg(a.name)) + "'>"
            + clipSvg + "<span class='name'>" + esc(a.name) + "</span><span class='size'>" + sizeLabel(a.size) + "</span></a>"
            + "<a class='save' href='" + appLink + "attachment?do=save&id=" + a.id + "' title='" + esc(qsTr("Save %1…").arg(a.name)) + "'>" + downloadSvg + "</a>"
            + (aiPanel.aiOn && aiPanel.attachmentOk(a) ? "<a class='save sum' href='" + appLink + "ai?do=attsum&id=" + a.id + "' title='" + esc(qsTr("Summarize %1").arg(a.name)) + "'>" + sparkSvg + "</a>" : "")
            + "</span>").join("")
        const all = list.length > 1
            ? "<a class='saveall' href='" + appLink + "attachment?do=saveall&ids=" + list.map(a => a.id).join(",") + "'>" + downloadSvg + esc(qsTr("Save all")) + "</a>"
            : ""
        return "<div class='atts'><div class='atthead'>" + esc(list.length === 1 ? qsTr("1 attachment") : qsTr("%1 attachments").arg(list.length)) + all + "</div>" + chips + "</div>"
    }
    function attachmentName(id) {
        for (const m of (lastMessages || []))
            for (const a of (m.attachments || [])) if (a.id === id) return a.name
        return ""
    }
    // a chip shows a spinner while its file downloads
    function setBusy(id, on) {
        web.runJavaScript("(function(){var e=document.getElementById('att-" + Number(id) + "');if(e)e.classList." + (on ? "add" : "remove") + "('busy')})()")
    }
    // scripted check: click the first element matching `selector`
    function clickFirst(selector) {
        web.runJavaScript("(function(){var e=document.querySelector(" + JSON.stringify(selector) + ");if(e)e.click();return !!e})()")
    }
    function query(url) {
        const out = {}
        const q = url.indexOf("?") >= 0 ? url.substring(url.indexOf("?") + 1) : ""
        for (const kv of q.split("&")) {
            const i = kv.indexOf("=")
            if (i > 0) out[kv.substring(0, i)] = decodeURIComponent(kv.substring(i + 1).replace(/\+/g, " "))
        }
        return out
    }
    function followAppLink(url) {
        const q = query(url)
        const path = url.substring(appLink.length).split("?")[0]
        if (path === "contact" && q.a) root.contact(q.a, q.n || "")
        else if (path === "attachment" && q.do === "open") root.openAttachment(Number(q.id))
        else if (path === "attachment" && q.do === "save") root.saveAttachment(Number(q.id), attachmentName(Number(q.id)))
        else if (path === "attachment" && q.do === "saveall") root.saveAttachments(q.ids.split(",").map(Number))
        else if (path === "ai" && q.do === "original") { showTr = false; aiPanel.trShown = false; rerender() }
        else if (path === "ai" && q.do === "attsum") aiPanel.summarizeAttachment(Number(q.id), attachmentName(Number(q.id)))
    }
    function css(c) { return String(c) }
    function fontStack() { return (Theme.font ? "'" + Theme.font + "'," : "") + "sans-serif" }
    function loading() {
        web.loadHtml("<html><body style=\"margin:0;background:" + css(Theme.window) + ";color:" + css(Theme.textMuted)
            + ";font-family:" + fontStack() + ";padding:40px\">" + qsTr("Loading…") + "</body></html>")
    }
    property var lastMessages: null
    function rerender() { if (lastMessages) showMessages(lastMessages) }
    Connections {
        target: Theme
        // after the whole palette has switched (Theme.apply sets the colours one by one)
        function onWindowChanged() { Qt.callLater(root.rerender) }
    }

    function showMessages(msgs) {
        lastMessages = msgs
        aiPanel.sync(msgs)
        if (translated && !aiPanel.trReady) { translated = null; showTr = false }
        let parts = []
        for (const m of msgs) {
            const tr = translated && showTr && m.id === translated.id ? translated : null
            const body = tr
                ? "<div dir='auto' style='white-space:pre-wrap;font-size:14px;line-height:1.5'>" + esc(tr.text) + "</div>"
                : m.html
                ? m.html
                : (m.fetched || m.text
                    ? "<pre style='white-space:pre-wrap;font-family:inherit;font-size:14px;margin:0'>" + esc(m.text || "") + "</pre>"
                    : "<p style='color:#8a8d96'>" + qsTr("Downloading message…") + "</p>")
            const banner = tr
                ? "<div class='aitr'>" + sparkSvg + "<span>" + esc(qsTr("Translated to %1 by the assistant").arg(tr.target)) + "</span><a href='" + appLink + "ai?do=original'>" + esc(qsTr("Show original")) + "</a></div>"
                : ""
            const doc = "<!doctype html><html><head><meta charset='utf-8'><base target='_blank'>"
                + "<style>body{margin:0;font-family:sans-serif;font-size:14px;color:#1d1d22;overflow-wrap:anywhere}img{max-width:100%;height:auto}</style>"
                + "</head><body>" + body + "</body></html>"
            const files = attachmentChips((m.attachments || []).filter(a => !a.inline))
            const fromName = m.from && m.from !== m.fromAddr ? m.from : ""
            const recipients = [[qsTr("To"), m.to], [qsTr("Cc"), m.cc], [qsTr("Bcc"), m.bcc]]
                .filter(r => (r[1] || []).length)
                .map(r => "<span class='rcpt'><span class='lbl'>" + r[0] + ":</span> " + people(r[1]) + "</span>")
                .join("")
            parts.push("<section class='msg'><header><div class='top'><div class='from'>"
                + personLink(m.fromAddr, fromName, "who", m.from)
                + (fromName ? " " + personLink(m.fromAddr, fromName, "addr", "<" + m.fromAddr + ">") : "") + "</div>"
                + "<span class='date'>" + esc(new Date(m.date).toLocaleString(Qt.locale(), "ddd d MMM yyyy, hh:mm")) + "</span></div>"
                + (recipients ? "<div class='to'>" + recipients + "</div>" : "") + "</header>"
                + banner + "<div class='paper'><iframe sandbox='allow-same-origin allow-popups' srcdoc=\"" + esc(doc) + "\"></iframe></div>"
                + files + "</section>")
        }
        const subject = msgs.length ? esc(msgs[msgs.length - 1].subject) : ""
        const page = "<!doctype html><html><head><meta charset='utf-8'><style>"
            + "body{margin:0;padding:18px 32px 40px;font-family:" + fontStack() + ";background:" + css(Theme.window) + ";color:" + css(Theme.text) + "}"
            + "h1{font-size:22px;font-weight:600;margin:0 0 14px;unicode-bidi:plaintext;color:" + css(Theme.textStrong) + "}"
            + ".msg{border-bottom:1px solid " + css(Theme.border) + ";padding:14px 0;animation:in .22s cubic-bezier(.2,.8,.2,1) both}"
            + ".msg:nth-of-type(2){animation-delay:.03s}.msg:nth-of-type(3){animation-delay:.06s}"
            + "@keyframes in{from{opacity:0;transform:translateY(6px)}to{opacity:1;transform:none}}"
            + "header{font-size:14px;margin-bottom:10px}.addr,.to{color:" + css(Theme.textMuted) + ";font-size:12px}"
            + ".top{display:flex;justify-content:space-between;gap:16px;align-items:baseline}"
            + ".date{flex:none;color:" + css(Theme.textMuted) + ";font-size:12px}"
            + ".paper{background:#fff;border-radius:" + (Theme.dark ? "10px;padding:14px 16px" : "0") + "}"
            + "iframe{border:0;width:100%;display:block}"
            + "a{color:inherit;text-decoration:none}"
            + ".from a.who{font-weight:600;color:" + css(Theme.textStrong) + "}"
            + "a.who,a.addr,a.person{border-radius:4px;cursor:pointer}"
            + "a.who:hover,a.addr:hover,a.person:hover{color:" + css(Theme.accent) + ";text-decoration:underline;text-underline-offset:2px}"
            + ".to{margin-top:2px;line-height:1.6}.rcpt{margin-right:12px}.rcpt .lbl{opacity:.8}.to a.person{color:" + css(Theme.textSubtle) + "}"
            + ".aitr{display:flex;align-items:center;gap:8px;font-size:12px;color:" + css(Theme.textMuted) + ";margin-bottom:8px}.aitr svg{color:" + css(Theme.accent) + "}"
            + ".aitr a{color:" + css(Theme.accent) + ";cursor:pointer}.aitr a:hover{text-decoration:underline}"
            + ".atts{margin-top:12px}.atthead{display:flex;align-items:center;gap:14px;font-size:12px;color:" + css(Theme.textMuted) + ";margin-bottom:8px}"
            + ".att{display:inline-flex;align-items:stretch;border:1px solid " + css(Theme.border) + ";border-radius:8px;margin:0 8px 8px 0;font-size:13px;color:" + css(Theme.text) + ";background:" + css(Theme.surface) + ";overflow:hidden;max-width:100%}"
            + ".att a{display:inline-flex;align-items:center;gap:8px;padding:7px 10px;cursor:pointer;transition:background .11s}"
            + ".att a:hover{background:" + css(Theme.hover) + "}.att a.save{border-left:1px solid " + css(Theme.border) + ";color:" + css(Theme.icon) + "}"
            + ".att a.save:hover{color:" + css(Theme.accent) + "}.att svg{flex:none;color:" + css(Theme.icon) + "}"
            + ".att .name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:280px}.att .size{color:" + css(Theme.textMuted) + ";font-size:11px}"
            + ".att.busy{opacity:.6;pointer-events:none}.att.busy .open svg{animation:spin 1s linear infinite}@keyframes spin{to{transform:rotate(360deg)}}"
            + "a.saveall{display:inline-flex;align-items:center;gap:5px;color:" + css(Theme.accent) + ";cursor:pointer}a.saveall svg{color:" + css(Theme.accent) + "}a.saveall:hover{text-decoration:underline}"
            + "</style></head><body><h1>" + subject + "</h1>" + parts.join("")
            + "<script>function fit(f){try{f.style.height=f.contentDocument.documentElement.scrollHeight+'px'}catch(e){}}"
            + "document.querySelectorAll('iframe').forEach(f=>{f.onload=()=>{fit(f);setTimeout(()=>fit(f),500);setTimeout(()=>fit(f),2000)}})</script>"
            + "</body></html>"
        web.loadHtml(page, "about:blank")
    }

    readonly property bool inInbox: ["inbox", "pinned", "snoozed", "folder", "contact"].indexOf(listName) >= 0

    Rectangle {
        id: bar
        width: parent.width
        height: 48
        color: Theme.window
        z: 1
        Row {
            x: 12
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4
            IconButton { icon: "back"; tip: qsTr("Close (Esc)"); onClicked: root.close() }
            Rectangle { width: 1; height: 22; color: Theme.border; anchors.verticalCenter: parent.verticalCenter }
            IconButton {
                readonly property bool isDone: root.listName === "done" || !!(root.thread && root.thread.done)
                icon: isDone ? "inbox" : "done"
                tip: isDone ? qsTr("Not done (E)") : qsTr("Done (E)")
                onClicked: root.action(isDone ? "undone" : "done")
            }
            IconButton {
                icon: "pin"
                active: root.thread && root.thread.pinned
                tip: root.thread && root.thread.pinned ? qsTr("Unpin (P)") : qsTr("Pin (P)")
                onClicked: root.action(root.thread && root.thread.pinned ? "unpin" : "pin")
            }
            IconButton {
                id: snoozeBtn
                icon: "clock"
                tip: root.listName === "snoozed" ? qsTr("Unsnooze") : qsTr("Snooze (S)")
                onClicked: root.listName === "snoozed" ? root.action("unsnooze") : root.snooze(snoozeBtn)
            }
            IconButton { icon: "unread"; tip: qsTr("Mark as unread (U)"); onClicked: root.action("markAsUnseen") }
            IconButton {
                visible: !root.inInbox
                icon: "inbox"
                tip: qsTr("Move to Inbox")
                onClicked: root.action("moveToInbox")
            }
            IconButton { visible: root.listName !== "archive"; icon: "archive"; tip: qsTr("Archive"); onClicked: root.action("archive") }
            IconButton { visible: root.listName !== "spam"; icon: "spam"; tip: qsTr("Spam (!)"); onClicked: root.action("markAsSpam") }
            IconButton {
                icon: "trash"
                tip: root.listName === "trash" ? qsTr("Delete forever") : qsTr("Delete (#)")
                onClicked: root.action(root.listName === "trash" ? "permanentDelete" : "moveToTrash")
            }
        }
        Row {
            anchors.right: parent.right
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4
            IconButton { icon: "reply"; tip: qsTr("Reply (R)"); onClicked: root.respond("reply") }
            IconButton { icon: "reply-all"; tip: qsTr("Reply all (A)"); onClicked: root.respond("replyAll") }
            IconButton { icon: "forward"; tip: qsTr("Forward (F)"); onClicked: root.respond("forward") }
        }
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
    }

    // quick reply bar at the bottom of the conversation
    Rectangle {
        id: quick
        anchors.bottom: parent.bottom
        width: parent.width
        height: 64
        color: Theme.window
        z: 1
        Rectangle { width: parent.width; height: 1; color: Theme.border }
        Rectangle {
            objectName: "quickReply"
            anchors.left: parent.left
            anchors.right: qr.left
            anchors.leftMargin: 20
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            height: 40
            radius: 20
            color: Theme.surface
            border.color: qh.hovered ? Theme.accent : Theme.border
            Behavior on border.color { ColorAnimation { duration: Theme.fast } }
            Row {
                x: 16
                anchors.verticalCenter: parent.verticalCenter
                spacing: 10
                IconImage { anchors.verticalCenter: parent.verticalCenter; width: 16; height: 16; sourceSize: Qt.size(32, 32); source: "qrc:/qt/qml/org/rustyinbox/icons/reply.svg"; color: Theme.textMuted }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.thread ? qsTr("Reply to %1…").arg(root.thread.from) : qsTr("Reply…")
                    font.pixelSize: 14
                    color: Theme.textMuted
                }
            }
            HoverHandler { id: qh; cursorShape: Qt.IBeamCursor }
            TapHandler { onTapped: root.respond("reply") }
        }
        Row {
            id: qr
            anchors.right: parent.right
            anchors.rightMargin: 16
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4
            IconButton { icon: "reply-all"; tip: qsTr("Reply all (A)"); onClicked: root.respond("replyAll") }
            IconButton { icon: "forward"; tip: qsTr("Forward (F)"); onClicked: root.respond("forward") }
        }
    }

    // AI: summary, translation, smart replies, phishing check (height 0 when AI is off)
    AiReaderPanel {
        id: aiPanel
        anchors.top: bar.bottom
        width: parent.width
        z: 1
        listName: root.listName
        maxHeight: Math.max(180, root.height * 0.55)
        onSmartReply: (text) => root.smartReply(text)
        onTranslationReady: (tr) => { root.translated = tr; root.showTr = true; aiPanel.trShown = true; root.rerender() }
        onToggleTranslation: { root.showTr = !root.showTr; aiPanel.trShown = root.showTr; root.rerender() }
        onAttachmentBusy: (id, busy) => root.setBusy(id, busy)
    }
    Shortcut {
        sequence: "Shift+S"
        enabled: root.visible && aiPanel.aiOn && (!root.Window.window || root.Window.window.keysOn !== false)
        onActivated: aiPanel.summarize()
    }

    WebEngineView {
        id: web
        anchors.top: aiPanel.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: quick.top
        settings.localContentCanAccessRemoteUrls: true
        settings.javascriptCanOpenWindows: false
        settings.pluginsEnabled: false
        backgroundColor: Theme.window

        onNavigationRequested: (request) => {
            if (request.url.toString().startsWith(root.appLink)) {
                request.reject()
                root.followAppLink(request.url.toString())
                return
            }
            const isPage = request.url.toString() === "about:blank" || request.url.toString().startsWith("data:")
            if (!isPage && request.navigationType === WebEngineNavigationRequest.LinkClickedNavigation) {
                Qt.openUrlExternally(request.url)
                request.reject()
            }
        }
        onNewWindowRequested: (request) => Qt.openUrlExternally(request.requestedUrl)
    }
}
