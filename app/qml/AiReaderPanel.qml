import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import org.rustyinbox

// Reader AI panel (features 6-10 and 16): a strip of actions above the conversation (Summarize,
// Translate, Suggest replies, Check for phishing) and the result cards below it. Every card has a
// loading, an error (with Retry) and a closed state; nothing is ever shown that the provider did not
// answer. Results of the automatic features (switches in Settings -> AI) appear in the same cards.
Item {
    id: root

    property var messages: []              // the conversation (Reader.lastMessages), oldest first
    property string listName: "inbox"
    property bool aiOn: false              // the manual strip shows (AI configured + reader tools on)
    property bool trShown: false           // the Reader shows the translation instead of the original
    property bool trReady: false           // a translation of the newest message exists
    signal smartReply(string text)
    signal translationReady(var tr)        // {id, text, target}
    signal toggleTranslation()
    signal attachmentBusy(int id, bool busy)

    readonly property int newestId: messages.length ? messages[messages.length - 1].id : 0
    // per feature: {state: idle|loading|done|error, id, partial, data, error}
    property var st: ({})
    property string convKey: ""
    property bool autoDone: false
    readonly property bool anyShown: ["summary", "translate", "replies", "phishing", "attachment"].some(k => {
        const s = (st[k] || {}).state
        return k === "translate" ? (s === "loading" || s === "error") : (s === "loading" || s === "done" || s === "error")
    })
    readonly property bool showing: aiOn || anyShown

    property real maxHeight: 420
    height: showing ? Math.min(col.implicitHeight, maxHeight) : 0
    visible: height > 0
    clip: true
    Behavior on height { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }

    function stateOf(k) { return (st[k] || {}).state || "idle" }
    function setSt(k, patch) {
        const o = Object.assign({}, st)
        o[k] = Object.assign({}, st[k] || {}, patch)
        st = o
    }
    function isRtl(s) {
        s = String(s || "")
        const letters = (s.match(/[A-Za-z\u0590-\u08FF]/g) || []).length
        const arabic = (s.match(/[\u0590-\u08FF]/g) || []).length
        return letters > 0 && arabic * 2 > letters
    }
    function bodyLength(m) {
        return String(m.text || String(m.html || "").replace(/<[^>]*>/g, "")).length
    }
    function bodyReady(m) { return !!(m && (m.fetched || m.text || m.html)) }

    Assistant {
        id: ai
        onPartial: (id, text) => { const k = root.featureOf(id); if (k) root.setSt(k, { partial: text }) }
        onFinished: (id, json) => {
            const k = root.featureOf(id)
            if (!k) return
            const r = JSON.parse(json)
            if (k === "translate") {
                root.setSt(k, { state: "done", id: 0 })
                root.trReady = true
                root.translationReady({ id: r.messageId, text: r.text, target: r.target })
                return
            }
            if (k === "attachment") root.attachmentBusy(root.st.attachment.attId, false)
            root.setSt(k, { state: "done", data: r, id: 0 })
        }
        onFailed: (id, message) => {
            const k = root.featureOf(id)
            if (!k) return
            if (k === "attachment") root.attachmentBusy(root.st.attachment.attId, false)
            root.setSt(k, { state: "error", error: message, id: 0 })
        }
    }
    function featureOf(id) {
        for (const k of ["summary", "translate", "replies", "phishing", "attachment"])
            if (st[k] && st[k].id === id && id) return k
        return ""
    }

    function enabled() { return ai.available() && ai.feature_on("aiReadTools") }

    // ---- running a feature -------------------------------------------------------------------------
    function start(k, starter, extra) {
        stop(k)
        setSt(k, Object.assign({ state: "loading", error: "", partial: "", data: null, id: 0 }, extra || {}))
        const id = starter()
        setSt(k, { id: id })
    }
    function stop(k) {
        const s = st[k]
        if (s && s.state === "loading" && s.id) {
            ai.cancel(s.id)
            if (k === "attachment") attachmentBusy(s.attId, false)
        }
    }
    function dismiss(k) {
        stop(k)
        setSt(k, { state: "idle", id: 0 })
    }
    function summarize() { if (newestId) start("summary", () => ai.summarize(newestId)) }
    function translate() {
        if (!newestId) return
        if (trReady) { toggleTranslation(); return }
        start("translate", () => ai.translate_message(newestId, ""))
    }
    function suggestReplies() { if (newestId) start("replies", () => ai.smart_replies(newestId)) }
    function checkPhishing() { if (newestId) start("phishing", () => ai.phishing(newestId)) }
    function summarizeAttachment(attId, name) {
        attachmentBusy(attId, true)
        start("attachment", () => ai.summarize_attachment(attId), { attId: attId, name: name })
    }
    function attachmentOk(a) { return ai.attachment_supported(a.mime || "", a.name || "") }

    // ---- a conversation was shown (or got its bodies) ------------------------------------------------
    function sync(msgs) {
        messages = msgs
        aiOn = enabled()
        const key = msgs.length ? msgs[0].id + ":" + msgs[msgs.length - 1].id : ""
        if (key !== convKey) {
            for (const k of Object.keys(st)) stop(k)
            st = ({})
            trReady = false
            trShown = false
            convKey = key
            autoDone = false
        }
        if (!autoDone && msgs.length && bodyReady(msgs[msgs.length - 1])) {
            autoDone = true
            runAutos(msgs)
        }
    }
    function runAutos(msgs) {
        if (!ai.available()) return
        const newest = msgs[msgs.length - 1]
        const own = listName === "sent" || listName === "drafts"
        if (ai.feature_on("aiAutoSummary") && (msgs.length >= 3 || bodyLength(newest) > 1500)) summarize()
        if (ai.feature_on("aiAutoReplies") && !own) suggestReplies()
        if (ai.feature_on("aiAutoPhishing") && !own && ai.sender_is_new(newest.id)) checkPhishing()
    }

    // ---- layout ------------------------------------------------------------------------------------
    component Card: Item {
        id: card
        property bool shown: false
        property string title
        property bool busy: false
        property color tone: Theme.border
        property color fill: Theme.surface
        default property alias content: inner.data
        signal closed()
        width: parent ? parent.width : 0
        height: shown ? frame.height + 8 : 0
        visible: height > 0
        clip: true
        Behavior on height { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
        Rectangle {
            id: frame
            x: 32
            width: parent.width - 64
            height: inside.implicitHeight + 24
            radius: 12
            color: card.fill
            border.color: card.tone
            Column {
                id: inside
                x: 14
                y: 12
                width: parent.width - 28
                spacing: 8
                Item {
                    width: parent.width
                    height: 22
                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 8
                        IconImage { anchors.verticalCenter: parent.verticalCenter; width: 15; height: 15; sourceSize: Qt.size(30, 30); source: "qrc:/qt/qml/org/rustyinbox/icons/sparkle.svg"; color: card.tone === Theme.border ? Theme.accent : card.tone }
                        Text { anchors.verticalCenter: parent.verticalCenter; text: card.title; font.pixelSize: 13; font.weight: Font.DemiBold; color: Theme.textStrong }
                        AiDots { visible: card.busy; anchors.verticalCenter: parent.verticalCenter }
                    }
                    IconButton { anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; size: 22; icon: "close"; tip: qsTr("Close"); onClicked: card.closed() }
                }
                Item { id: inner; width: parent.width; height: childrenRect.height }
            }
        }
    }
    // loading text / error + Retry shared by the cards
    component Failure: Row {
        property string message
        signal retry()
        spacing: 12
        width: parent ? parent.width : 0
        Text { width: parent.width - retryBtn.width - 12; text: parent.message; wrapMode: Text.Wrap; font.pixelSize: 13; color: Theme.danger; anchors.verticalCenter: parent.verticalCenter }
        AiButton { id: retryBtn; text: qsTr("Retry"); primary: true; anchors.verticalCenter: parent.verticalCenter; onClicked: parent.retry() }
    }

    Flickable {
        anchors.fill: parent
        contentWidth: width
        contentHeight: col.implicitHeight
        interactive: contentHeight > height + 1
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        ScrollBar.vertical: ScrollBar {}
    Column {
        id: col
        width: parent.width

        // the strip
        Item {
            width: parent.width
            height: root.aiOn ? 44 : 0
            visible: height > 0
            clip: true
            Row {
                x: 32
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                IconImage { anchors.verticalCenter: parent.verticalCenter; width: 16; height: 16; sourceSize: Qt.size(32, 32); source: "qrc:/qt/qml/org/rustyinbox/icons/sparkle.svg"; color: Theme.accent }
                AiPill { objectName: "aiSummarize"; text: qsTr("Summarize"); tip: qsTr("Summary and action items (Shift+S)"); active: root.stateOf("summary") !== "idle"; busy: root.stateOf("summary") === "loading"; onClicked: root.stateOf("summary") === "idle" ? root.summarize() : root.dismiss("summary") }
                AiPill { objectName: "aiTranslate"; text: root.trReady ? (root.trShown ? qsTr("Show original") : qsTr("Show translation")) : qsTr("Translate"); tip: qsTr("Translate the newest message"); active: root.trShown; busy: root.stateOf("translate") === "loading"; onClicked: root.translate() }
                AiPill { objectName: "aiReplies"; text: qsTr("Suggest replies"); tip: qsTr("Three short replies to choose from"); active: root.stateOf("replies") !== "idle"; busy: root.stateOf("replies") === "loading"; onClicked: root.stateOf("replies") === "idle" ? root.suggestReplies() : root.dismiss("replies") }
                AiPill { objectName: "aiPhishing"; text: qsTr("Check for phishing"); tip: qsTr("Look for signs of a scam in the newest message"); active: root.stateOf("phishing") !== "idle"; busy: root.stateOf("phishing") === "loading"; onClicked: root.stateOf("phishing") === "idle" ? root.checkPhishing() : root.dismiss("phishing") }
            }
        }

        // summary + action items
        Card {
            objectName: "aiSummaryCard"
            shown: root.stateOf("summary") !== "idle"
            title: qsTr("Summary")
            busy: root.stateOf("summary") === "loading"
            onClosed: root.dismiss("summary")
            Column {
                width: parent.width
                spacing: 10
                Text {
                    visible: root.stateOf("summary") === "loading"
                    text: qsTr("Reading the conversation…")
                    font.pixelSize: 13; color: Theme.textMuted
                }
                Failure { visible: root.stateOf("summary") === "error"; message: (root.st.summary || {}).error || ""; onRetry: root.summarize() }
                Text {
                    visible: root.stateOf("summary") === "done"
                    width: parent.width
                    text: root.stateOf("summary") === "done" ? root.st.summary.data.summary : ""
                    wrapMode: Text.Wrap
                    font.pixelSize: 14
                    lineHeight: 1.2
                    color: Theme.text
                    horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                }
                Column {
                    visible: root.stateOf("summary") === "done"
                    width: parent.width
                    spacing: 6
                    Text {
                        text: root.stateOf("summary") === "done" && root.st.summary.data.actions.length > 0 ? qsTr("Action items") : qsTr("No action items")
                        font.pixelSize: 12; font.weight: Font.DemiBold; color: Theme.textMuted
                    }
                    Repeater {
                        model: root.stateOf("summary") === "done" ? root.st.summary.data.actions : []
                        delegate: Row {
                            id: act
                            required property var modelData
                            width: col.width - 64 - 28
                            spacing: 10
                            Rectangle { y: 3; width: 14; height: 14; radius: 4; color: "transparent"; border.color: Theme.icon; border.width: 1.5 }
                            Column {
                                width: parent.width - 24
                                spacing: 1
                                Text {
                                    width: parent.width
                                    text: act.modelData.task
                                    wrapMode: Text.Wrap
                                    font.pixelSize: 14; color: Theme.text
                                    horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                                }
                                Text {
                                    visible: text !== ""
                                    width: parent.width
                                    text: [act.modelData.owner, act.modelData.due ? qsTr("due %1").arg(act.modelData.due) : ""].filter(x => x).join("  ·  ")
                                    wrapMode: Text.Wrap
                                    font.pixelSize: 12; color: act.modelData.due ? Theme.accent : Theme.textMuted
                                }
                            }
                        }
                    }
                }
            }
        }

        // translation in progress / failed (a finished translation shows in the message itself)
        Card {
            objectName: "aiTranslateCard"
            shown: root.stateOf("translate") === "loading" || root.stateOf("translate") === "error"
            title: qsTr("Translate")
            busy: root.stateOf("translate") === "loading"
            onClosed: root.dismiss("translate")
            Column {
                width: parent.width
                Text {
                    visible: root.stateOf("translate") === "loading"
                    width: parent.width
                    height: Math.min(implicitHeight, 90)
                    clip: true
                    text: (root.st.translate || {}).partial || qsTr("Translating the newest message…")
                    wrapMode: Text.Wrap
                    font.pixelSize: 13; color: Theme.textSubtle
                    horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                }
                Failure { visible: root.stateOf("translate") === "error"; message: (root.st.translate || {}).error || ""; onRetry: root.translate() }
            }
        }

        // smart replies
        Card {
            objectName: "aiRepliesCard"
            shown: root.stateOf("replies") !== "idle"
            title: qsTr("Suggested replies")
            busy: root.stateOf("replies") === "loading"
            onClosed: root.dismiss("replies")
            Column {
                width: parent.width
                spacing: 8
                Text { visible: root.stateOf("replies") === "loading"; text: qsTr("Thinking of replies…"); font.pixelSize: 13; color: Theme.textMuted }
                Failure { visible: root.stateOf("replies") === "error"; message: (root.st.replies || {}).error || ""; onRetry: root.suggestReplies() }
                Repeater {
                    model: root.stateOf("replies") === "done" ? root.st.replies.data.replies : []
                    delegate: Rectangle {
                        id: chip
                        required property string modelData
                        objectName: "aiReplyChip"
                        width: parent.width
                        height: label.implicitHeight + 16
                        radius: 10
                        color: rh.hovered ? Theme.selected : "transparent"
                        border.color: rh.hovered ? Theme.accent : Theme.border
                        Behavior on color { ColorAnimation { duration: Theme.fast } }
                        Behavior on border.color { ColorAnimation { duration: Theme.fast } }
                        Row {
                            x: 12
                            y: 8
                            width: parent.width - 24
                            spacing: 10
                            IconImage { y: 2; width: 15; height: 15; sourceSize: Qt.size(30, 30); source: "qrc:/qt/qml/org/rustyinbox/icons/reply.svg"; color: Theme.icon }
                            Text {
                                id: label
                                width: parent.width - 25
                                text: chip.modelData
                                wrapMode: Text.Wrap
                                font.pixelSize: 14; color: Theme.text
                                horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                            }
                        }
                        HoverHandler { id: rh; cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: root.smartReply(chip.modelData) }
                    }
                }
            }
        }

        // phishing verdict: the colour says how worried to be
        Card {
            id: phCard
            objectName: "aiPhishingCard"
            readonly property string risk: root.stateOf("phishing") === "done" ? root.st.phishing.data.risk : ""
            readonly property color riskColor: risk === "high" ? Theme.danger : (risk === "medium" ? Theme.avatars[2] : Theme.avatars[3])
            shown: root.stateOf("phishing") !== "idle"
            title: risk === "high" ? qsTr("High risk of phishing") : risk === "medium" ? qsTr("Suspicious message") : risk === "low" ? qsTr("Looks safe") : qsTr("Phishing check")
            busy: root.stateOf("phishing") === "loading"
            tone: risk === "" ? Theme.border : riskColor
            fill: risk === "" ? Theme.surface : Qt.tint(Theme.window, Qt.rgba(riskColor.r, riskColor.g, riskColor.b, 0.13))
            onClosed: root.dismiss("phishing")
            Column {
                width: parent.width
                spacing: 6
                Text { visible: root.stateOf("phishing") === "loading"; text: qsTr("Checking the sender, links and wording…"); font.pixelSize: 13; color: Theme.textMuted }
                Failure { visible: root.stateOf("phishing") === "error"; message: (root.st.phishing || {}).error || ""; onRetry: root.checkPhishing() }
                Repeater {
                    model: root.stateOf("phishing") === "done" ? root.st.phishing.data.reasons : []
                    delegate: Row {
                        id: reason
                        required property string modelData
                        width: parent.width
                        spacing: 8
                        Rectangle { y: 7; width: 5; height: 5; radius: 2.5; color: phCard.riskColor }
                        Text {
                            width: parent.width - 14
                            text: reason.modelData
                            wrapMode: Text.Wrap
                            font.pixelSize: 14; color: Theme.text
                            horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                        }
                    }
                }
                Text {
                    visible: root.stateOf("phishing") === "done"
                    width: parent.width
                    text: qsTr("Based on the sender, links and wording. Server checks (SPF, DKIM) are not available here.")
                    wrapMode: Text.Wrap
                    font.pixelSize: 11; color: Theme.textMuted
                }
            }
        }

        // attachment summary
        Card {
            objectName: "aiAttachmentCard"
            shown: root.stateOf("attachment") !== "idle"
            title: qsTr("Summary of %1").arg((root.st.attachment || {}).name || "")
            busy: root.stateOf("attachment") === "loading"
            onClosed: root.dismiss("attachment")
            Column {
                width: parent.width
                spacing: 6
                Text {
                    visible: root.stateOf("attachment") === "loading"
                    width: parent.width
                    height: Math.min(implicitHeight, 160)
                    clip: true
                    text: (root.st.attachment || {}).partial || qsTr("Reading the file…")
                    wrapMode: Text.Wrap
                    font.pixelSize: 14; color: Theme.textSubtle
                    horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                }
                Failure { visible: root.stateOf("attachment") === "error"; message: (root.st.attachment || {}).error || ""; onRetry: root.summarizeAttachment(root.st.attachment.attId, root.st.attachment.name) }
                Text {
                    visible: root.stateOf("attachment") === "done"
                    width: parent.width
                    text: root.stateOf("attachment") === "done" ? root.st.attachment.data.text : ""
                    wrapMode: Text.Wrap
                    font.pixelSize: 14
                    lineHeight: 1.2
                    color: Theme.text
                    horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                }
            }
        }

        Item { width: 1; height: root.showing ? 4 : 0 }
    }
    }
}
