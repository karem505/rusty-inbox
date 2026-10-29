import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

// Writing assistant of the composer (features 1-5): a menu (proofread, rewrite, translate, write
// for me, suggest subject) and a result sheet over the bottom of the editor. Nothing changes the
// draft until Replace or Insert below is clicked; Replace and Insert only touch the user's own text
// (the part above the signature and the quoted message).
Item {
    id: root

    property var composer                  // the Composer: ownText(cb), applyOwn(text, mode), draft, notice(text)
    property int bottomInset: 80           // keeps the sheet above the attachment row and the bottom bar
    property var subjects: []              // suggested subject lines (shown by AiSubjectChips)
    property alias menu: aiMenu
    readonly property bool active: phase !== "idle"

    // idle | input | loading | result | error
    property string phase: "idle"
    property string kind: ""               // proofread | rewrite | translate | write | subject
    property string title: ""
    property string inputKind: ""          // write | language
    property int reqId: 0
    property string partialText: ""
    property string resultText: ""
    property string diffHtml: ""
    property bool changed: true
    property string errorText: ""
    property var retryFn: null
    readonly property bool rtl: isRtl(phase === "result" ? resultText : partialText)

    function isRtl(s) {
        s = String(s || "")
        const letters = (s.match(/[A-Za-z\u0590-\u08FF]/g) || []).length
        const arabic = (s.match(/[\u0590-\u08FF]/g) || []).length
        return letters > 0 && arabic * 2 > letters
    }
    // AI is configured and the composer tools are switched on
    function enabled() { return ai.available() && ai.feature_on("aiComposeTools") }
    function account() { return (composer && composer.draft && composer.draft.account) || "" }

    function openMenu() {
        if (!composer) return
        aiMenu.popup(menuAnchor, 0, menuAnchor.height + 4)
    }
    // the item the menu pops up from (the sparkle button in the toolbar)
    property Item menuAnchor: root

    Assistant {
        id: ai
        onPartial: (id, text) => { if (id === root.reqId && root.phase === "loading") root.partialText = text }
        onFinished: (id, json) => {
            if (id !== root.reqId) return
            const r = JSON.parse(json)
            root.reqId = 0
            if (r.feature === "subject") {
                root.subjects = r.subjects || []
                root.phase = "idle"
                return
            }
            root.resultText = r.text || ""
            root.diffHtml = r.diffHtml || ""
            root.changed = r.changed !== false
            root.phase = "result"
        }
        onFailed: (id, message) => {
            if (id !== root.reqId) return
            root.reqId = 0
            root.errorText = message
            root.phase = "error"
        }
    }

    // ---- running a tool ---------------------------------------------------------------------------
    function run(kindName, titleText, starter) {
        cancelCurrent()
        kind = kindName
        title = titleText
        partialText = ""
        resultText = ""
        diffHtml = ""
        errorText = ""
        retryFn = () => root.run(kindName, titleText, starter)
        phase = "loading"
        reqId = starter()
    }
    function cancelCurrent() {
        if (phase === "loading" && reqId) ai.cancel(reqId)
        reqId = 0
    }
    function dismiss() {
        cancelCurrent()
        phase = "idle"
    }
    // read the user's own text first; an empty draft has nothing to work on
    function withText(kindName, titleText, fn) {
        if (!composer) return
        composer.ownText(function(text) {
            if (!text.trim()) { composer.notice(qsTr("Write something first, then use the assistant")); return }
            root.run(kindName, titleText, () => fn(text))
        })
    }
    function doProofread() { withText("proofread", qsTr("Proofread"), t => ai.proofread(t, account())) }
    function doRewrite(style, label) { withText("rewrite", qsTr("Rewrite: %1").arg(label), t => ai.rewrite(t, style, account())) }
    function doTranslate(language) { withText("translate", qsTr("Translate to %1").arg(language), t => ai.translate_draft(t, language, account())) }
    function doSubjects() {
        if (!composer) return
        composer.ownText(function(text) {
            if (!text.trim()) { composer.notice(qsTr("Write a few lines first, then ask for subject ideas")); return }
            root.run("subject", qsTr("Suggesting subjects"), () => ai.subject_ideas(text, root.account()))
        })
    }
    function askWrite() {
        cancelCurrent()
        kind = "write"
        inputKind = "write"
        title = qsTr("Write for me")
        inputField.text = ""
        phase = "input"
        inputField.forceActiveFocus()
    }
    function askLanguage() {
        cancelCurrent()
        kind = "translate"
        inputKind = "language"
        title = qsTr("Translate to another language")
        langField.text = ""
        phase = "input"
        langField.forceActiveFocus()
    }
    function startWrite(instruction) {
        if (!instruction.trim()) return
        const source = (composer && composer.draft && composer.draft.sourceMessage) || 0
        root.run("write", qsTr("Write for me"), () => ai.write_for_me(instruction, source, root.account()))
    }
    function retry() { if (retryFn) retryFn() }
    function apply(mode) {
        composer.applyOwn(root.resultText, mode)
        phase = "idle"
        composer.notice(mode === "replace" ? qsTr("Draft updated") : qsTr("Text added to the draft"))
    }
    function copyResult() {
        clip.text = root.resultText
        clip.selectAll()
        clip.copy()
        composer.notice(qsTr("Copied"))
    }
    TextEdit { id: clip; visible: false }

    // ---- menu -------------------------------------------------------------------------------------
    Menu {
        id: aiMenu
        MenuItem { text: qsTr("Proofread"); onTriggered: root.doProofread() }
        Menu {
            title: qsTr("Rewrite")
            MenuItem { text: qsTr("Formal"); onTriggered: root.doRewrite("formal", qsTr("Formal")) }
            MenuItem { text: qsTr("Friendly"); onTriggered: root.doRewrite("friendly", qsTr("Friendly")) }
            MenuItem { text: qsTr("Shorter"); onTriggered: root.doRewrite("shorter", qsTr("Shorter")) }
            MenuItem { text: qsTr("Longer"); onTriggered: root.doRewrite("longer", qsTr("Longer")) }
            MenuItem { text: qsTr("Clearer"); onTriggered: root.doRewrite("clearer", qsTr("Clearer")) }
            MenuItem { text: qsTr("More persuasive"); onTriggered: root.doRewrite("persuasive", qsTr("More persuasive")) }
        }
        Menu {
            title: qsTr("Translate")
            MenuItem { text: qsTr("English"); onTriggered: root.doTranslate("English") }
            MenuItem { text: qsTr("Arabic"); onTriggered: root.doTranslate("Arabic") }
            MenuItem { text: qsTr("Other…"); onTriggered: root.askLanguage() }
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Write for me…"); onTriggered: root.askWrite() }
        MenuItem { text: qsTr("Suggest subject"); onTriggered: root.doSubjects() }
    }

    // ---- the sheet --------------------------------------------------------------------------------
    Rectangle {
        id: sheet
        objectName: "aiSheet"
        visible: opacity > 0
        opacity: root.active ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
        transform: Translate { y: root.active ? 0 : 14; Behavior on y { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } } }
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        anchors.bottomMargin: root.bottomInset
        height: Math.min(root.height - root.bottomInset - 120, layout.implicitHeight + 28)
        radius: 12
        color: Theme.surface
        border.color: root.phase === "error" ? Theme.danger : Theme.border
        // a click inside the sheet must not reach the editor behind it
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: (w) => w.accepted = true }

        ColumnLayout {
            id: layout
            anchors.fill: parent
            anchors.margins: 14
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                IconImage { Layout.preferredWidth: 16; Layout.preferredHeight: 16; sourceSize: Qt.size(32, 32); source: "qrc:/qt/qml/org/rustyinbox/icons/sparkle.svg"; color: Theme.accent }
                Text { text: root.title; font.pixelSize: 14; font.weight: Font.DemiBold; color: Theme.textStrong }
                AiDots { visible: root.phase === "loading"; Layout.alignment: Qt.AlignVCenter }
                Item { Layout.fillWidth: true }
                AiButton { visible: root.phase === "loading"; text: qsTr("Cancel"); flat: true; onClicked: root.dismiss() }
                IconButton { visible: root.phase !== "loading"; size: 24; icon: "close"; tip: qsTr("Close (Esc)"); onClicked: root.dismiss() }
            }

            // instruction for "Write for me"
            Rectangle {
                visible: root.phase === "input" && root.inputKind === "write"
                Layout.fillWidth: true
                Layout.preferredHeight: 92
                radius: 8
                color: Theme.window
                border.color: inputField.activeFocus ? Theme.accent : Theme.border
                ScrollView {
                    anchors.fill: parent
                    anchors.margins: 2
                    TextArea {
                        id: inputField
                        wrapMode: TextArea.Wrap
                        placeholderText: qsTr("Say what the email should do, or paste bullet points…")
                        placeholderTextColor: Theme.textMuted
                        font.pixelSize: 14
                        color: Theme.text
                        background: Item {}
                        horizontalAlignment: root.isRtl(text) ? Text.AlignRight : Text.AlignLeft
                        Keys.onPressed: (e) => {
                            if ((e.key === Qt.Key_Return || e.key === Qt.Key_Enter) && (e.modifiers & Qt.ControlModifier)) {
                                e.accepted = true
                                root.startWrite(text)
                            }
                        }
                    }
                }
            }
            // language for "Translate > Other…"
            Rectangle {
                visible: root.phase === "input" && root.inputKind === "language"
                Layout.fillWidth: true
                Layout.preferredHeight: 36
                radius: 8
                color: Theme.window
                border.color: langField.activeFocus ? Theme.accent : Theme.border
                TextField {
                    id: langField
                    anchors.fill: parent
                    placeholderText: qsTr("Language, for example French")
                    placeholderTextColor: Theme.textMuted
                    font.pixelSize: 14
                    color: Theme.text
                    background: Item {}
                    onAccepted: if (text.trim() !== "") root.doTranslate(text.trim())
                }
            }

            // streamed text, the highlighted changes, or the error
            ScrollView {
                id: bodyScroll
                visible: root.phase === "loading" || root.phase === "result" || root.phase === "error"
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(body.implicitHeight + 4, 230)
                clip: true
                contentWidth: availableWidth
                Text {
                    id: body
                    objectName: "aiSheetText"
                    width: bodyScroll.availableWidth
                    wrapMode: Text.Wrap
                    font.pixelSize: 14
                    lineHeight: 1.25
                    horizontalAlignment: root.rtl && root.phase !== "error" ? Text.AlignRight : Text.AlignLeft
                    textFormat: root.phase === "result" && root.kind === "proofread" && root.changed ? Text.RichText : Text.PlainText
                    color: root.phase === "error" ? Theme.danger : Theme.text
                    text: {
                        if (root.phase === "error") return root.errorText
                        if (root.phase === "loading") return root.partialText !== "" ? root.partialText : (root.kind === "subject" ? "" : qsTr("Working…"))
                        if (root.kind === "proofread") return root.changed ? root.styledDiff(root.diffHtml) : qsTr("No changes needed. The text already reads correctly.")
                        return root.resultText
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                visible: root.phase !== "loading"
                Item { Layout.fillWidth: true }
                AiButton { visible: root.phase === "input"; text: root.inputKind === "write" ? qsTr("Write") : qsTr("Translate"); primary: true; icon: "sparkle"
                    onClicked: root.inputKind === "write" ? root.startWrite(inputField.text) : (langField.text.trim() !== "" && root.doTranslate(langField.text.trim())) }
                AiButton { visible: root.phase === "error"; text: qsTr("Retry"); primary: true; onClicked: root.retry() }
                AiButton { visible: root.phase === "result" && (root.kind !== "proofread" || root.changed); objectName: "aiReplace"; text: qsTr("Replace"); primary: true; onClicked: root.apply("replace") }
                AiButton { visible: root.phase === "result" && root.kind !== "proofread"; text: qsTr("Insert below"); onClicked: root.apply("below") }
                AiButton { visible: root.phase === "result" && (root.kind !== "proofread" || root.changed); text: qsTr("Copy"); onClicked: root.copyResult() }
                AiButton { text: root.phase === "result" && (root.kind !== "proofread" || root.changed) ? qsTr("Cancel") : qsTr("Close"); flat: true; onClicked: root.dismiss() }
            }
        }
    }

    // <del>/<ins> from the backend become coloured spans (Qt's rich text has no ins/del styling)
    function styledDiff(html) {
        const del = Qt.tint(Theme.surface, Qt.rgba(Theme.danger.r, Theme.danger.g, Theme.danger.b, 0.28))
        const ins = Qt.tint(Theme.surface, Qt.rgba(0.25, 0.7, 0.4, 0.34))
        return String(html)
            .replace(/<del>/g, "<span style=\"background-color:" + del + ";text-decoration:line-through\">")
            .replace(/<ins>/g, "<span style=\"background-color:" + ins + "\">")
            .replace(/<\/(del|ins)>/g, "</span>")
    }
}
