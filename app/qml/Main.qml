import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rustyinbox
import Qt.labs.platform as Platform

ApplicationWindow {
    id: win
    width: 1440
    height: 900
    visible: true
    title: backend.app_name()
    color: Theme.window
    font.family: Theme.font !== "" ? Theme.font : Qt.application.font.family

    // built-in controls (menus, tooltips, scroll bars) follow the theme too
    palette.window: Theme.window
    palette.windowText: Theme.text
    palette.base: Theme.list
    palette.alternateBase: Theme.surface
    palette.text: Theme.text
    palette.button: Theme.surface
    palette.buttonText: Theme.text
    palette.highlight: Theme.accent
    palette.highlightedText: Theme.accentText
    palette.toolTipBase: Theme.toast
    palette.toolTipText: Theme.toastText
    palette.light: Theme.hover
    palette.mid: Theme.border
    palette.dark: Theme.textMuted
    palette.placeholderText: Theme.textMuted

    property var spec: ({ list: "inbox" })
    property var threads: []
    property var sidebarModel: ({ inbox: 0, pinned: 0, snoozed: 0, drafts: 0, categories: [], accounts: [] })
    readonly property var core: backend
    property int openId: -1
    // preferences from Settings (kept in sync through settings_changed)
    property var prefs: JSON.parse(backend.settings_json())
    readonly property bool smart: prefs.smartInbox === true && spec.list === "inbox" && !spec.category && !spec.folder

    // undo: deferred moves wait here for `undoMs` before reaching the engine
    readonly property int undoMs: (prefs.undoSeconds || 5) * 1000
    property var pending: []        // {action, ids, until, due}
    property var undoStack: []      // {kind: "deferred"|"inverse", action, ids, until}
    property var hidden: ({})       // thread id -> true while its deferred move is pending

    Backend {
        id: backend
        onTheme_changed: (json) => Theme.apply(JSON.parse(json))
        onSettings_changed: (json) => win.prefs = JSON.parse(json)
        onMail_changed: win.reload()
        onAction_done: (action) => win.reload()
        onSend_result: (id, subject, error) => {
            if (error === "") toast.show(subject ? qsTr("Sent: %1").arg(subject) : qsTr("Message sent"), false)
            else toast.show(qsTr("Not sent (%1). It is waiting under Scheduled.").arg(error), false)
        }
        onThread_loaded: (threadId, json) => {
            if (threadId === win.openId && readerLoader.item)
                readerLoader.item.showMessages(JSON.parse(json))
            if (threadId === win.openId && win.pendingRespond !== "") {
                const msgs = JSON.parse(json)
                const kind = win.pendingRespond
                win.pendingRespond = ""
                if (msgs.length) Qt.callLater(() => win.compose(kind, msgs[msgs.length - 1].id))
            }
        }
        onOpen_requested: (threadId) => {
            win.showWindow()
            win.select({ list: "inbox" })
            win.openThread(threadId)
        }
    }

    function reload() {
        const all = JSON.parse(backend.threads_json(JSON.stringify(spec), 500))
        threads = all.filter(t => !hidden[t.id])
        sidebarModel = JSON.parse(backend.sidebar_json())
    }

    function select(s) {
        spec = s
        closeThread()
        threadList.clearSelection()
        threadList.focusIndex = 0
        reload()
    }

    function openThread(id) {
        if (id < 0 || (spec.list === "drafts" && pendingRespond === "")) { openForEditing(id); return }
        if (composerLoader.active) closeComposer()
        openId = id
        readerLoader.active = true
        if (readerLoader.item) readerLoader.item.loading()
        backend.open_thread(id)
        const row = threads.find(t => t.id === id)
        if (row && row.unread && prefs.markReadOnOpen !== false) perform("markAsSeen", [id], 0, "", true)
    }

    function closeThread() {
        openId = -1
        readerLoader.active = false // releases the web engine renderer
    }

    function labelFor(action, n) {
        const s = n > 1 ? qsTr("%1 conversations").arg(n) : qsTr("Conversation")
        return ({
            done: qsTr("Marked as Done"), undone: qsTr("Moved to Inbox"), archive: qsTr("Archived"),
            moveToTrash: qsTr("Moved to Trash"), markAsSpam: qsTr("Marked as Spam"), moveToInbox: qsTr("Moved to Inbox"),
            permanentDelete: qsTr("Deleted forever"), pin: qsTr("Pinned"), unpin: qsTr("Unpinned"),
            markAsUnseen: qsTr("Marked as unread"), markAsSeen: qsTr("Marked as read"), snooze: qsTr("Snoozed"),
            unsnooze: qsTr("Unsnoozed")
        })[action] || action
    }

    // Optimistic local change so the list reacts before the engine reports back.
    function applyLocally(action, ids) {
        const set = {}
        for (const i of ids) set[i] = true
        const patch = ({ pin: { pinned: true }, unpin: { pinned: false }, markAsSeen: { unread: false }, markAsUnseen: { unread: true } })[action]
        if (patch) threads = threads.map(t => set[t.id] ? Object.assign({}, t, patch) : t)
    }

    function perform(action, ids, until, label, silent) {
        ids = (ids || []).filter(i => i >= 0)
        if (!ids.length) return
        const msg = label || labelFor(action, ids.length)
        if (backend.is_deferred(action)) {
            if (openId >= 0 && ids.indexOf(openId) >= 0) {
                const next = threadList.neighbourOf(openId)
                if (next >= 0) openThread(next); else closeThread()
            }
            const h = Object.assign({}, hidden)
            for (const i of ids) h[i] = true
            hidden = h
            const entry = { kind: "deferred", action: action, ids: ids, until: until || 0, due: Date.now() + undoMs }
            pending = pending.concat([entry])
            undoStack = undoStack.concat([entry])
            threadList.clearSelection()
            threads = threads.filter(t => !hidden[t.id])
            if (!silent) toast.show(msg, true)
            return
        }
        applyLocally(action, ids)
        backend.act(action, JSON.stringify(ids), until || 0)
        const inv = backend.inverse_of(action)
        if (!silent) {
            if (inv && !(inv === "snooze" && !until)) undoStack = undoStack.concat([{ kind: "inverse", action: inv, ids: ids, until: 0 }])
            toast.show(msg, inv !== "" && inv !== "snooze")
        }
        if (action === "snooze" && ids.indexOf(openId) >= 0) closeThread()
    }

    function undo() {
        if (!undoStack.length) return
        const e = undoStack[undoStack.length - 1]
        undoStack = undoStack.slice(0, -1)
        if (e.kind === "send") {
            const d = backend.unqueue(e.id)
            if (!d) { toast.show(qsTr("Too late, it was sent"), false); return }
            openComposer(JSON.parse(d), e.id)
            toast.show(e.later ? qsTr("Schedule removed") : qsTr("Sending undone"), false)
            return
        }
        if (e.kind === "deferred") {
            if (pending.indexOf(e) < 0) { toast.show(qsTr("Too late to undo"), false); return }
            pending = pending.filter(p => p !== e)
            const h = Object.assign({}, hidden)
            for (const i of e.ids) delete h[i]
            hidden = h
            reload()
        } else {
            applyLocally(e.action, e.ids)
            backend.act(e.action, JSON.stringify(e.ids), e.until || 0)
        }
        toast.show(qsTr("Undone"), false)
    }

    function commitDue(all) {
        const now = Date.now()
        const due = pending.filter(p => all || p.due <= now)
        if (!due.length) return
        pending = pending.filter(p => due.indexOf(p) < 0)
        for (const p of due) {
            if (all) backend.act_now(p.action, JSON.stringify(p.ids), p.until)
            else backend.act(p.action, JSON.stringify(p.ids), p.until)
        }
    }
    Timer { interval: 250; repeat: true; running: win.pending.length > 0; onTriggered: win.commitDue(false) }

    // Closing the window keeps the app in the status bar (sync and notifications continue);
    // "Quit" in the tray menu really exits. Without a tray (no StatusNotifier host) close quits.
    property bool quitting: false
    onClosing: (close) => {
        if (!quitting && tray.available && prefs.closeToTray !== false && autotestDir === "" && !stress) {
            close.accepted = false
            win.hide()
            return
        }
        commitDue(true)
    }
    function quitApp() {
        quitting = true
        commitDue(true)
        Qt.quit()
    }
    function showWindow() {
        win.show()
        win.raise()
        win.requestActivate()
    }

    Platform.SystemTrayIcon {
        id: tray
        visible: available
        // re-evaluated on theme switches (Theme colours) and when the unread count changes
        icon.source: { Theme.text; Theme.accent; return backend.tray_icon_url(win.sidebarModel.inbox > 0 && win.prefs.trayUnreadDot !== false) }
        tooltip: win.sidebarModel.inbox > 0
            ? qsTr("%1 · %2 unread").arg(backend.app_name()).arg(win.sidebarModel.inbox)
            : backend.app_name()
        onActivated: (reason) => {
            if (reason === Platform.SystemTrayIcon.Context) return
            if (win.visible && win.active) win.hide(); else win.showWindow()
        }
        menu: Platform.Menu {
            Platform.MenuItem { text: qsTr("Open %1").arg(backend.app_name()); onTriggered: win.showWindow() }
            Platform.MenuItem { text: qsTr("Check for new mail"); onTriggered: backend.sync_now() }
            Platform.MenuItem { text: qsTr("Settings…"); onTriggered: { win.showWindow(); win.openSettings("general") } }
            Platform.MenuSeparator {}
            Platform.MenuItem { text: qsTr("Quit"); onTriggered: win.quitApp() }
        }
    }

    function snoozeTargets(ids, anchor) {
        snoozeMenu.targetIds = ids
        snoozeMenu.popup(anchor, 0, anchor.height)
    }

    // ---- composing ------------------------------------------------------------------------------
    property bool composeExpanded: false
    property string pendingRespond: ""     // reply asked for before the thread finished loading
    function openComposer(draft, outboxId) {
        closeThread()
        composeExpanded = false
        composerLoader.active = true
        composerLoader.item.load(draft, outboxId || 0)
        composerLoader.item.forceActiveFocus()
    }
    function compose(kind, messageId) {
        if (composerLoader.active) return
        const acc = spec.account || ""
        openComposer(JSON.parse(backend.compose_json(kind, messageId || 0, acc)), 0)
    }
    // reply / reply all / forward to the newest message of the open (or focused) conversation
    function respond(kind) {
        if (composerLoader.active) return
        const r = readerLoader.item
        const msgs = r && r.lastMessages
        if (openId >= 0 && msgs && msgs.length) {
            compose(kind, msgs[msgs.length - 1].id)
        } else {
            const id = threadList.focusedId()
            if (id < 0) return
            pendingRespond = kind
            openThread(id)
        }
    }
    function closeComposer() {
        composerLoader.active = false
        composeExpanded = false
        reload()
    }
    function queueSend(draft, outboxId, sendAt) {
        const later = sendAt > 0
        const at = later ? sendAt : Date.now() + undoMs
        const r = JSON.parse(backend.queue_send(outboxId, JSON.stringify(draft), at))
        if (r.error) { toast.show(r.error, false); return }
        closeComposer()
        const entry = { kind: "send", id: r.id, later: later }
        undoStack = undoStack.concat([entry])
        toast.show(later ? qsTr("Scheduled for %1").arg(new Date(at).toLocaleString(Qt.locale(), "ddd d MMM, hh:mm")) : qsTr("Sending…"), true)
    }
    // open a row of the Drafts or Scheduled list in the composer
    function openForEditing(threadId) {
        if (threadId < 0) {
            const d = backend.unqueue(-threadId)
            if (d) { openComposer(JSON.parse(d), -threadId); reload() }
            else toast.show(qsTr("It is being sent right now"), false)
            return
        }
        const mid = backend.latest_message_id(threadId)
        if (mid > 0) openComposer(JSON.parse(backend.compose_json("draft", mid, "")), 0)
    }

    function openSettings(page) {
        settingsPage.page = page
        settingsPage.shown = true
        settingsPage.forceActiveFocus()
    }
    function listTitle(s) {
        const names = ({ inbox: qsTr("Smart Inbox"), pinned: qsTr("Pinned"), snoozed: qsTr("Snoozed"), done: qsTr("Done"),
                         sent: qsTr("Sent"), drafts: qsTr("Drafts"), scheduled: qsTr("Scheduled"), archive: qsTr("Archive"), spam: qsTr("Spam"), trash: qsTr("Trash") })
        let t = s.folder ? qsTr("Folder") : (names[s.list] || s.list)
        if (s.list === "inbox" && !prefs.smartInbox && !s.category) t = qsTr("Inbox")
        if (s.category) t = ({ personal: qsTr("People"), notifications: qsTr("Notifications"), newsletters: qsTr("Newsletters"), invitations: qsTr("Invitations") })[s.category] || s.category
        if (s.account) {
            const a = sidebarModel.accounts.find(x => x.email === s.account)
            t += "  ·  " + (a && a.name ? a.name : s.account)
        }
        return t
    }

    Component.onCompleted: {
        Theme.apply(JSON.parse(backend.theme_json()))
        threadList.collapsed = prefs.collapseAutomated === false ? ({}) : ({ notifications: true, newsletters: true })
        reload()
        backend.start()
    }

    // ---- keyboard ---------------------------------------------------------------------------
    // single-key shortcuts only while the mail view has the keyboard (not in Settings or while
    // writing: the composer handles its own keys)
    readonly property bool keysOn: !settingsPage.shown && !composerLoader.active
    Shortcut { sequences: ["C", "Ctrl+N"]; enabled: !settingsPage.shown && !composerLoader.active; onActivated: win.compose("new", 0) }
    Shortcut { sequence: "R"; enabled: win.keysOn; onActivated: win.respond("reply") }
    Shortcut { sequence: "A"; enabled: win.keysOn; onActivated: win.respond("replyAll") }
    Shortcut { sequence: "F"; enabled: win.keysOn; onActivated: win.respond("forward") }
    Shortcut { sequence: "Escape"; enabled: win.keysOn; onActivated: threadList.selecting ? threadList.clearSelection() : win.closeThread() }
    Shortcut { sequence: "X"; enabled: win.keysOn; onActivated: threadList.toggleFocused() }
    Shortcut { sequences: ["Ctrl+,", StandardKey.Preferences]; onActivated: win.openSettings("general") }
    Shortcut { sequences: ["Down", "J"]; enabled: win.keysOn; onActivated: { if (threadList.moveFocus(1) && win.openId >= 0) win.openThread(threadList.focusedId()) } }
    Shortcut { sequences: ["Up", "K"]; enabled: win.keysOn; onActivated: { if (threadList.moveFocus(-1) && win.openId >= 0) win.openThread(threadList.focusedId()) } }
    Shortcut { sequences: ["Return", "O"]; enabled: win.keysOn; onActivated: win.openThread(threadList.focusedId()) }
    Shortcut { sequence: "E"; enabled: win.keysOn; onActivated: win.perform(win.spec.list === "done" ? "undone" : "done", threadList.targetIds()) }
    Shortcut { sequences: ["#", "Delete"]; enabled: win.keysOn; onActivated: win.perform(win.spec.list === "trash" ? "permanentDelete" : "moveToTrash", threadList.targetIds()) }
    Shortcut { sequence: "!"; enabled: win.keysOn; onActivated: win.perform("markAsSpam", threadList.targetIds()) }
    Shortcut {
        sequence: "P"
        enabled: win.keysOn
        onActivated: {
            const ids = threadList.targetIds()
            const r = ids.length ? threadList.rowOf(ids[0]) : null
            win.perform(r && r.pinned ? "unpin" : "pin", ids)
        }
    }
    Shortcut {
        sequence: "U"
        enabled: win.keysOn
        onActivated: {
            const ids = threadList.targetIds()
            const r = ids.length ? threadList.rowOf(ids[0]) : null
            win.perform(r && r.unread ? "markAsSeen" : "markAsUnseen", ids)
        }
    }
    Shortcut { sequence: "S"; enabled: win.keysOn; onActivated: win.snoozeTargets(threadList.targetIds(), readerLoader.active ? readerLoader : threadList) }
    Shortcut { sequences: [StandardKey.Undo, "Z"]; enabled: win.keysOn; onActivated: win.undo() }
    Shortcut { sequence: StandardKey.SelectAll; enabled: win.keysOn; onActivated: threadList.selectAll() }

    SnoozeMenu {
        id: snoozeMenu
        property var targetIds: []
        onPicked: (untilMs, label) => win.perform("snooze", targetIds, untilMs, qsTr("Snoozed until %1").arg(new Date(untilMs).toLocaleString(Qt.locale(), "ddd d MMM, hh:mm")))
    }

    Item {
        id: page
        anchors.fill: parent

        RowLayout {
            anchors.fill: parent
            spacing: 0
            // Settings covers the mail view; pointer handlers below it would still see taps
            // (TapHandler only grabs passively), so switch the mail view off meanwhile
            enabled: !settingsPage.shown

            Sidebar {
                id: sidebar
                visible: !(composerLoader.active && win.composeExpanded)
                onCompose: win.compose("new", 0)
                appName: backend.app_name()
                onOpenSettings: (pg) => win.openSettings(pg)
                Layout.preferredWidth: 280
                Layout.fillHeight: true
                model: win.sidebarModel
                spec: win.spec
                onSelect: (s) => win.select(s)
            }

            ThreadList {
                id: threadList
                visible: !(composerLoader.active && win.composeExpanded)
                Layout.preferredWidth: readerLoader.active || composerLoader.active ? 440 : -1
                Layout.fillWidth: !readerLoader.active && !composerLoader.active
                Layout.fillHeight: true
                threads: win.threads
                smart: win.smart
                title: win.listTitle(win.spec)
                compact: win.prefs.density === "compact"
                showAvatars: win.prefs.showAvatars !== false
                showPreview: win.prefs.showPreview !== false
                openId: win.openId
                onOpen: (id) => win.openThread(id)
                onAction: (name, ids) => win.perform(name, ids)
                onSnooze: (ids, anchor) => win.snoozeTargets(ids, anchor)
            }

            Rectangle { Layout.preferredWidth: 1; Layout.fillHeight: true; color: Theme.border; visible: readerLoader.active || (composerLoader.active && !win.composeExpanded) }

            // composer: replaces the reader (or takes the whole window when expanded)
            Loader {
                id: composerLoader
                Layout.fillWidth: true
                Layout.fillHeight: true
                active: false
                visible: active
                sourceComponent: Composer {
                    backend: win.core
                    accounts: win.sidebarModel.accounts
                    expanded: win.composeExpanded
                    onClosed: win.closeComposer()
                    onNotice: (text) => toast.show(text, false)
                    onToggleExpand: win.composeExpanded = !win.composeExpanded
                    onSendRequested: (draft, outboxId, sendAt) => win.queueSend(draft, outboxId, sendAt)
                }
                onLoaded: composerIn.restart()
                ParallelAnimation {
                    id: composerIn
                    NumberAnimation { target: composerLoader.item; property: "opacity"; from: 0; to: 1; duration: Theme.normal; easing.type: Theme.easing }
                    NumberAnimation { target: composerLoader.item; property: "y"; from: 24; to: 0; duration: Theme.normal; easing.type: Theme.easing }
                }
            }

            // reader: the web engine exists only while a thread is open
            Loader {
                id: readerLoader
                Layout.fillWidth: true
                Layout.fillHeight: true
                active: false
                visible: active
                source: "Reader.qml"
                onLoaded: { item.loading(); readerIn.restart() }
                // slide + fade in when a conversation opens
                ParallelAnimation {
                    id: readerIn
                    NumberAnimation { target: readerLoader.item; property: "opacity"; from: 0; to: 1; duration: Theme.normal; easing.type: Theme.easing }
                    NumberAnimation { target: readerLoader.item; property: "x"; from: 32; to: 0; duration: Theme.normal; easing.type: Theme.easing }
                }
                Binding { when: readerLoader.item; target: readerLoader.item; property: "listName"; value: win.spec.folder ? "folder" : win.spec.list }
                Binding { when: readerLoader.item; target: readerLoader.item; property: "thread"; value: win.threads.find(t => t.id === win.openId) || null }
                Connections {
                    target: readerLoader.item
                    function onAction(name) { win.perform(name, [win.openId]) }
                    function onSnooze(anchor) { win.snoozeTargets([win.openId], anchor) }
                    function onClose() { win.closeThread() }
                    function onRespond(kind) { win.respond(kind) }
                }
            }
        }

        // Settings: slides over the mail view
        Settings {
            id: settingsPage
            property bool shown: false
            anchors.fill: parent
            backend: win.core
            visible: opacity > 0
            opacity: shown ? 1 : 0
            scale: shown ? 1 : 0.985
            Behavior on opacity { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
            Behavior on scale { NumberAnimation { duration: Theme.normal; easing.type: Theme.easing } }
            onClosed: shown = false
            onNotice: (text) => toast.show(text, false)
            z: 5
        }

        Toast {
            id: toast
            z: 6
            lift: composerLoader.active ? 64 : 0
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            timeout: win.undoMs
            onUndo: win.undo()
        }
    }

    // ---- scripted check: --autotest=<dir> (always offline, see main.rs) -------------------------
    readonly property string autotestDir: {
        const a = Qt.application.arguments.find(x => x.startsWith("--autotest="))
        return a ? a.substring(11) : ""
    }
    property bool firstFrame: false
    onFrameSwapped: if (!firstFrame) { firstFrame = true; if (autotestDir !== "") console.warn("AUTOTEST firstframe", threads.length) }

    function shot(name, then) {
        page.grabToImage(r => { r.saveToFile(autotestDir + "/" + name + ".png"); console.warn("AUTOTEST shot", name); if (then) then() })
    }
    function log(what) {
        console.warn("AUTOTEST", what, JSON.stringify({ threads: threads.length, inbox: sidebarModel.inbox, pinned: sidebarModel.pinned, snoozed: sidebarModel.snoozed, pending: pending.length, open: openId }))
    }
    property int step: 0
    property var picked: ({})
    // --stress: reload the list every 40 ms for 20 s, flipping data each time (mimics sync passes
    // landing while the list is on screen), then quit
    readonly property bool stress: Qt.application.arguments.indexOf("--stress") >= 0
    property int stressN: 0
    Timer {
        running: win.stress
        interval: 60
        repeat: true
        onTriggered: {
            if (++win.stressN > 200) { console.warn("AUTOTEST stress done", win.stressN); Qt.quit(); return }
            const all = JSON.parse(backend.threads_json(JSON.stringify(win.spec), 500))
            // drop/reorder a few rows each round so rows are added, removed and moved
            // odd rounds: worst case (a seventh of rows removed/re-added, a third flip unread);
            // even rounds: what a sync pass usually delivers (same data, maybe one new row)
            const k = win.stressN % 7
            win.threads = win.stressN % 2
                ? all.filter((t, i) => i % 7 !== k).map(t => Object.assign({}, t, { unread: (t.id + win.stressN) % 3 === 0 }))
                : all
            win.sidebarModel = JSON.parse(backend.sidebar_json())
            if (win.stressN % 25 === 0) { const c = Object.assign({}, threadList.collapsed); c.notifications = !c.notifications; threadList.collapsed = c }
        }
    }
    Timer {
        id: script
        running: win.autotestDir !== ""
        interval: 1500
        repeat: true
        onTriggered: {
            const people = () => win.threads.filter(t => !t.pinned && (t.category === "personal" || !t.category))
            switch (win.step++) {
            case 0: win.log("start"); win.shot("1-smart-inbox"); break
            case 1: win.picked.a = people()[0].id; win.openThread(win.picked.a); break
            case 2: break
            case 3: win.shot("2-reader"); break
            case 4: win.perform("pin", [win.picked.a]); break
            case 5: win.log("after pin"); win.closeThread(); break
            case 6: win.picked.b = people()[0].id; win.perform("done", [win.picked.b]); win.log("done pending"); win.shot("3-undo-toast"); break
            case 7: win.undo(); break
            case 8: win.log("after undo (b back: " + !!win.threads.find(t => t.id === win.picked.b) + ")"); break
            case 9: win.picked.c = people()[0].id; win.perform("snooze", [win.picked.c], Date.now() + 86400000, ""); break
            case 10: win.log("after snooze"); break
            case 11: win.picked.d = people()[0].id; win.perform("moveToTrash", [win.picked.d]); break
            case 12: case 13: case 14: case 15: break
            case 16: win.log("after trash commit (d in store: " + !!JSON.parse(backend.threads_json(JSON.stringify(win.spec), 500)).find(t => t.id === win.picked.d) + ")"); break
            case 17: win.select({ list: "snoozed" }); break
            case 18: win.log("snoozed list"); win.shot("4-snoozed"); break
            case 19: win.select({ list: "inbox", category: "notifications" }); break
            case 20: win.log("notifications"); win.shot("5-notifications"); break
            case 21: sidebar.tapList({ list: "sent" }); break
            case 22: win.log("sent all accounts (unfolded: " + sidebar.listExpanded("sent") + ")"); win.shot("6-sent-unfolded"); break
            case 23: win.select({ list: "sent", account: win.sidebarModel.accounts[0].email }); break
            case 24: win.log("sent one account " + win.listTitle(win.spec)); win.shot("7-sent-one-account"); break
            case 25: win.select({ list: "inbox" }); threadList.selectMode = true; break
            case 26: win.log("select mode " + threadList.selecting); win.shot("8-select-mode"); break
            case 27: threadList.selectAll(); break
            case 28: win.log("select all " + threadList.selectedIds.length + "/" + threadList.visibleCount); win.shot("9-select-all"); break
            case 29: threadList.clearSelection(); win.openSettings("general"); break
            case 30: win.shot("10-settings-general", () => settingsPage.page = "accounts"); break
            case 31: win.shot("11-settings-accounts", () => settingsPage.page = "addAccount"); break
            case 32: win.shot("12-settings-add-account", () => settingsPage.page = "appearance"); break
            case 33: settingsPage.save("density", "compact"); settingsPage.save("themeMode", "light"); break
            case 34: win.log("prefs " + JSON.stringify(win.prefs)); win.shot("13-settings-appearance-light", () => settingsPage.page = "notifications"); break
            case 35: win.shot("14-settings-notifications", () => settingsPage.page = "shortcuts"); break
            case 36: win.shot("15-settings-shortcuts", () => settingsPage.page = "about"); break
            case 37: win.shot("16-settings-about", () => settingsPage.closed()); break
            case 38: break
            case 39: win.shot("17-compact-light-list", () => { settingsPage.save("density", "comfortable"); settingsPage.save("themeMode", "system") }); break
            case 40: win.select({ list: "inbox" }); win.picked.r = people()[0].id; win.openThread(win.picked.r); break
            case 41: break
            case 42: win.respond("reply"); break
            case 43: win.log("reply composer " + composerLoader.active + " to=" + JSON.stringify(composerLoader.item.draft.to) + " subject=" + composerLoader.item.draft.subject); win.shot("18-reply"); break
            case 44: composerLoader.item.closeComposer(); break
            case 45: win.log("after close (drafts kept locally)"); win.compose("new", 0); break
            case 46: composerLoader.item.fillForTest([{ name: "Test Person", address: "test@example.org" }, { name: null, address: "bad address" }], "Autotest message", "<p>Hello <b>there</b></p>"); break
            case 47: win.shot("19-new-message"); composerLoader.item.send(0); break
            case 48: win.log("bad address kept composer open " + composerLoader.active); composerLoader.item.fillForTest([{ name: "Test Person", address: "test@example.org" }], "Autotest message", "<p>Hello <b>there</b></p>"); break
            case 49: composerLoader.item.send(0); break
            case 50: win.log("queued, composer closed " + !composerLoader.active); win.shot("20-sending"); break
            case 51: case 52: case 53: case 54: break
            case 55: win.select({ list: "scheduled" }); break
            case 56: win.log("scheduled list " + win.threads.length + " first=" + (win.threads[0] ? win.threads[0].preview : "")); win.shot("21-scheduled"); break
            case 57: if (win.threads.length) win.openThread(win.threads[0].id); break
            case 58: win.log("reopened failed send in composer " + composerLoader.active + " to=" + JSON.stringify(composerLoader.item.draft.to)); break
            case 59: win.shot("22-edit-failed"); break
            case 60: composerLoader.item.discard(); break
            case 61: win.log("discarded: scheduled now " + win.threads.length + ", composer " + composerLoader.active); win.shot("23-after-discard"); break
            case 62: console.warn("AUTOTEST end"); Qt.quit(); break
            }
        }
    }
}
