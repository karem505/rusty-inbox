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

    function esc(s) {
        return String(s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;")
    }
    function names(list) {
        return (list || []).map(a => a.name || a.mailbox || a.address || a.email || a).filter(x => typeof x === "string").join(", ")
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
        let parts = []
        for (const m of msgs) {
            const body = m.html
                ? m.html
                : (m.fetched || m.text
                    ? "<pre style='white-space:pre-wrap;font-family:inherit;font-size:14px;margin:0'>" + esc(m.text || "") + "</pre>"
                    : "<p style='color:#8a8d96'>" + qsTr("Downloading message…") + "</p>")
            const doc = "<!doctype html><html><head><meta charset='utf-8'><base target='_blank'>"
                + "<style>body{margin:0;font-family:sans-serif;font-size:14px;color:#1d1d22;overflow-wrap:anywhere}img{max-width:100%;height:auto}</style>"
                + "</head><body>" + body + "</body></html>"
            const files = (m.attachments || []).filter(a => !a.inline)
                .map(a => "<span class='att'>" + esc(a.name) + "</span>").join("")
            parts.push("<section class='msg'><header><div class='top'><div><b>" + esc(m.from) + "</b> <span class='addr'>&lt;" + esc(m.fromAddr) + "&gt;</span></div>"
                + "<span class='date'>" + esc(new Date(m.date).toLocaleString(Qt.locale(), "ddd d MMM yyyy, hh:mm")) + "</span></div>"
                + "<div class='to'>" + qsTr("To") + ": " + esc(names(m.to)) + (names(m.cc) ? " · " + qsTr("Cc") + ": " + esc(names(m.cc)) : "") + "</div></header>"
                + "<div class='paper'><iframe sandbox='allow-same-origin allow-popups' srcdoc=\"" + esc(doc) + "\"></iframe></div>"
                + (files ? "<div class='atts'>" + files + "</div>" : "") + "</section>")
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
            + ".atts{margin-top:10px}.att{display:inline-block;border:1px solid " + css(Theme.border) + ";border-radius:8px;padding:6px 10px;margin:0 6px 6px 0;font-size:12px;color:" + css(Theme.text) + "}"
            + "</style></head><body><h1>" + subject + "</h1>" + parts.join("")
            + "<script>function fit(f){try{f.style.height=f.contentDocument.documentElement.scrollHeight+'px'}catch(e){}}"
            + "document.querySelectorAll('iframe').forEach(f=>{f.onload=()=>{fit(f);setTimeout(()=>fit(f),500);setTimeout(()=>fit(f),2000)}})</script>"
            + "</body></html>"
        web.loadHtml(page, "about:blank")
    }

    readonly property bool inInbox: ["inbox", "pinned", "snoozed", "folder"].indexOf(listName) >= 0

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

    WebEngineView {
        id: web
        anchors.top: bar.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: quick.top
        settings.localContentCanAccessRemoteUrls: true
        settings.javascriptCanOpenWindows: false
        settings.pluginsEnabled: false
        backgroundColor: Theme.window

        onNavigationRequested: (request) => {
            const isPage = request.url.toString() === "about:blank" || request.url.toString().startsWith("data:")
            if (!isPage && request.navigationType === WebEngineNavigationRequest.LinkClickedNavigation) {
                Qt.openUrlExternally(request.url)
                request.reject()
            }
        }
        onNewWindowRequested: (request) => Qt.openUrlExternally(request.requestedUrl)
    }
}
