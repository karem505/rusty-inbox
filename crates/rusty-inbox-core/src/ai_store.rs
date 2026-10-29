//! Storage behind the AI organizing features (labels, follow-ups, briefings, runner state) and the
//! SQL that decides which mail counts as "needs reply" and "follow up".
//!
//! Rules (docs/ai/DESIGN.md):
//! - the rule-based category in `messages.category` is never overwritten; an AI label overrides it
//!   only while [`Store::set_ai_labels_applied`] is on, only for messages the rules left as
//!   `personal`, and only from senders the user never wrote to. Turning the feature off restores the
//!   rule category at once because nothing was changed.
//! - "needs reply" ends by itself when the user replies (a later message of theirs in the thread),
//!   marks the conversation Done, or says "Not needed" ([`Store::ai_dismiss_thread`]).
//! - a follow-up ends when somebody else writes in the thread after the sent message.

use crate::store::{lit, Store};
use crate::CoreResult;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::sync::atomic::Ordering;

/// SQL: a later message of the user (Sent folder or one of their addresses) exists in the thread of
/// message alias `m`.
pub(crate) fn replied_after(m: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM messages s JOIN folders fs ON fs.id=s.folder_id WHERE s.thread_id={m}.thread_id \
         AND s.date_ms>{m}.date_ms AND COALESCE(fs.role,'') NOT IN ('drafts','trash','spam') \
         AND (fs.role='sent' OR lower(s.from_addr) IN (SELECT lower(email) FROM accounts)))"
    )
}

/// SQL: somebody else wrote in the thread of `m` after it.
pub(crate) fn answered_by_them(m: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM messages s JOIN folders fs ON fs.id=s.folder_id WHERE s.thread_id={m}.thread_id \
         AND s.date_ms>{m}.date_ms AND COALESCE(fs.role,'') NOT IN ('sent','drafts','trash','spam') \
         AND lower(s.from_addr) NOT IN (SELECT lower(email) FROM accounts))"
    )
}

/// SQL: inbox message `m` is labelled "needs reply" and still open.
pub(crate) fn needs_reply_cond(m: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM ai_labels nl WHERE nl.message_id={m}.id AND nl.needs_reply=1 AND nl.dismissed=0) \
         AND {m}.answered=0 AND NOT {})",
        replied_after(m)
    )
}

/// SQL: sent message `m` was judged to expect an answer and none came.
pub(crate) fn follow_up_cond(m: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM ai_followups fu WHERE fu.message_id={m}.id AND fu.expects=1 AND fu.dismissed=0) \
         AND NOT {})",
        answered_by_them(m)
    )
}

/// SQL: `account` column of alias `m` is one of `allowed`.
fn account_in(m: &str, allowed: &[String]) -> String {
    if allowed.is_empty() {
        return "0".into();
    }
    format!("{m}.account IN ({})", allowed.iter().map(|a| lit(a)).collect::<Vec<_>>().join(","))
}

/// A stored message as the AI jobs read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiMessage {
    pub id: i64,
    pub thread_id: i64,
    pub account: String,
    pub folder_role: String,
    pub from_name: Option<String>,
    pub from_addr: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date_ms: i64,
    /// plain text of the body (or the preview when the body is not downloaded)
    pub body: String,
    pub attachments: Vec<String>,
    /// the rule-based category
    pub category: String,
    pub seen: bool,
}

/// What the model decided about one message (input of [`Store::ai_save_labels`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiLabelIn {
    pub message_id: i64,
    pub category: String,
    pub needs_reply: bool,
    pub priority: u8,
    pub reason: String,
}

/// A stored label.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiLabelRow {
    pub message_id: i64,
    pub category: String,
    /// the override may be shown (rule category was personal, sender never written to)
    pub eligible: bool,
    pub needs_reply: bool,
    pub dismissed: bool,
    pub priority: u8,
    pub reason: String,
    pub model: String,
}

/// A sent message waiting to be checked or notified about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiFollowUp {
    pub message_id: i64,
    pub thread_id: i64,
    pub subject: String,
    pub awaiting: String,
}

/// Counts shown next to the sidebar entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiCounts {
    pub needs_reply: i64,
    pub follow_up: i64,
}

fn people(json: &str) -> String {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(json) else { return String::new() };
    list.iter()
        .filter_map(|a| {
            let addr = a["mailbox"].as_str().or(a["address"].as_str()).unwrap_or_default();
            let name = a["name"].as_str().or(a["displayName"]["value"].as_str()).unwrap_or_default().trim();
            match (name.is_empty() || name == addr, addr.is_empty()) {
                (_, true) if name.is_empty() => None,
                (_, true) => Some(name.to_string()),
                (true, false) => Some(addr.to_string()),
                (false, false) => Some(format!("{name} <{addr}>")),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

const MESSAGE_COLUMNS: &str =
    "m.id, COALESCE(m.thread_id,0), m.account, COALESCE(f.role,''), m.from_name, m.from_addr, \
     m.to_json, m.cc_json, m.subject, m.date_ms, m.body_text, m.body_html, m.preview, m.category, m.seen";

fn message_of(r: &rusqlite::Row) -> rusqlite::Result<AiMessage> {
    let text: Option<String> = r.get(10)?;
    let html: Option<String> = r.get(11)?;
    let preview: String = r.get(12)?;
    let body = match (text.filter(|t| !t.trim().is_empty()), html.filter(|h| !h.trim().is_empty())) {
        (Some(t), _) => t,
        (None, Some(h)) => crate::mail::mime::html_to_text(&h),
        (None, None) => preview,
    };
    Ok(AiMessage {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        account: r.get(2)?,
        folder_role: r.get(3)?,
        from_name: r.get(4)?,
        from_addr: r.get(5)?,
        to: people(&r.get::<_, String>(6)?),
        cc: people(&r.get::<_, String>(7)?),
        subject: r.get(8)?,
        date_ms: r.get(9)?,
        body,
        attachments: vec![],
        category: r.get(13)?,
        seen: r.get::<_, i64>(14)? != 0,
    })
}

impl Store {
    /// Switch the AI category override on or off for every list and count (feature "Smart filtering").
    pub fn set_ai_labels_applied(&self, on: bool) {
        self.ai_apply.store(on, Ordering::SeqCst);
    }

    pub fn ai_labels_applied(&self) -> bool {
        self.ai_apply.load(Ordering::SeqCst)
    }

    /// SQL expression for the category shown for message alias `m` joined to `ai_labels` alias `l`.
    pub(crate) fn eff_category(&self, m: &str, l: &str) -> String {
        if self.ai_labels_applied() {
            format!("CASE WHEN {l}.eligible=1 AND {m}.category='personal' THEN {l}.category ELSE {m}.category END")
        } else {
            format!("{m}.category")
        }
    }

    // ---------- runner state ----------

    pub fn ai_state(&self, key: &str) -> Option<String> {
        self.db
            .lock()
            .query_row("SELECT value FROM ai_state WHERE key=?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn ai_set_state(&self, key: &str, value: &str) -> CoreResult<()> {
        self.db.lock().execute(
            "INSERT INTO ai_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn ai_clear_state(&self, key: &str) -> CoreResult<()> {
        self.db.lock().execute("DELETE FROM ai_state WHERE key=?1", [key])?;
        Ok(())
    }

    // ---------- labels (features 11, 12) ----------

    fn messages_where(&self, cond: &str, limit: usize) -> CoreResult<Vec<AiMessage>> {
        let db = self.db.lock();
        let mut st = db.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages m JOIN folders f ON f.id=m.folder_id \
             LEFT JOIN threads t ON t.id=m.thread_id WHERE {cond} LIMIT {limit}"
        ))?;
        let mut rows: Vec<AiMessage> = st.query_map([], message_of)?.collect::<Result<_, _>>()?;
        let mut ast = db.prepare("SELECT filename FROM attachments WHERE message_id=?1 AND inline=0")?;
        for m in rows.iter_mut() {
            m.attachments = ast.query_map([m.id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        }
        Ok(rows)
    }

    /// One stored message for the AI jobs.
    pub fn ai_message(&self, id: i64) -> CoreResult<Option<AiMessage>> {
        Ok(self.messages_where(&format!("m.id={id}"), 1)?.into_iter().next())
    }

    /// Inbox mail not labelled yet, newest first: from `allowed` accounts, dated `since_ms` or later,
    /// not from the user. The rules' `personal` mail always qualifies, `invitations` only with
    /// `with_invitations` (needs-reply detection); automated and bulk mail is left alone.
    pub fn ai_unlabeled(
        &self,
        allowed: &[String],
        since_ms: i64,
        with_invitations: bool,
        limit: usize,
    ) -> CoreResult<Vec<AiMessage>> {
        let cats = if with_invitations { "('personal','invitations')" } else { "('personal')" };
        self.messages_where(
            &format!(
                "f.role='inbox' AND m.draft=0 AND m.date_ms>={since_ms} AND m.category IN {cats} AND {} \
                 AND NOT EXISTS (SELECT 1 FROM ai_labels l WHERE l.message_id=m.id) \
                 AND lower(m.from_addr) NOT IN (SELECT lower(email) FROM accounts) \
                 ORDER BY m.date_ms DESC",
                account_in("m", allowed)
            ),
            limit,
        )
    }

    /// Store the model's labels. A label may override the rule category only when the rules said
    /// `personal` and the user never wrote to that sender; either way it is kept for "needs reply".
    /// A "Not needed" the user already gave stays.
    pub fn ai_save_labels(&self, labels: &[AiLabelIn], model: &str, now_ms: i64) -> CoreResult<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        for l in labels {
            let row: Option<(String, String)> = tx
                .query_row("SELECT category, lower(from_addr) FROM messages WHERE id=?1", [l.message_id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            let Some((rule, from)) = row else { continue };
            let wrote_to = !from.is_empty()
                && tx.query_row(
                    &format!(
                        "SELECT EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id=m.folder_id \
                         WHERE f.role='sent' AND {})",
                        crate::store::recipient_condition(&from)
                    ),
                    [],
                    |r| r.get::<_, bool>(0),
                )?;
            let eligible = rule == "personal" && !wrote_to;
            tx.execute(
                "INSERT INTO ai_labels(message_id,category,eligible,needs_reply,dismissed,priority,reason,model,at_ms)
                 VALUES(?1,?2,?3,?4,0,?5,?6,?7,?8)
                 ON CONFLICT(message_id) DO UPDATE SET category=excluded.category, eligible=excluded.eligible,
                   needs_reply=excluded.needs_reply, priority=excluded.priority, reason=excluded.reason,
                   model=excluded.model, at_ms=excluded.at_ms",
                params![l.message_id, l.category, eligible, l.needs_reply, l.priority, l.reason, model, now_ms],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Mark messages as looked at without a label (the model left them out): they are not asked again.
    pub fn ai_save_skipped(&self, ids: &[i64], now_ms: i64) -> CoreResult<()> {
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        for id in ids {
            tx.execute(
                "INSERT OR IGNORE INTO ai_labels(message_id,category,eligible,needs_reply,dismissed,priority,reason,model,at_ms)
                 SELECT id, category, 0, 0, 0, 1, '', 'skipped', ?2 FROM messages WHERE id=?1",
                params![id, now_ms],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn ai_label(&self, message_id: i64) -> CoreResult<Option<AiLabelRow>> {
        Ok(self
            .db
            .lock()
            .query_row(
                "SELECT message_id,category,eligible,needs_reply,dismissed,priority,reason,model FROM ai_labels \
                 WHERE message_id=?1",
                [message_id],
                |r| {
                    Ok(AiLabelRow {
                        message_id: r.get(0)?,
                        category: r.get(1)?,
                        eligible: r.get(2)?,
                        needs_reply: r.get(3)?,
                        dismissed: r.get(4)?,
                        priority: r.get(5)?,
                        reason: r.get(6)?,
                        model: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// "Not needed": clears needs reply and the follow-up of a conversation.
    pub fn ai_dismiss_thread(&self, thread_id: i64) -> CoreResult<()> {
        let db = self.db.lock();
        db.execute(
            "UPDATE ai_labels SET dismissed=1 WHERE message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
            [thread_id],
        )?;
        db.execute(
            "UPDATE ai_followups SET dismissed=1 WHERE message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
            [thread_id],
        )?;
        Ok(())
    }

    /// Counts for the sidebar entries (same conditions as the two lists).
    pub fn ai_counts(&self) -> CoreResult<AiCounts> {
        let db = self.db.lock();
        let needs_reply = db.query_row(
            &format!(
                "SELECT COUNT(DISTINCT m.thread_id) FROM messages m JOIN folders f ON f.id=m.folder_id \
                 JOIN threads t ON t.id=m.thread_id WHERE f.role='inbox' AND t.done=0 AND t.snoozed_until IS NULL \
                 AND {}",
                needs_reply_cond("m")
            ),
            [],
            |r| r.get(0),
        )?;
        let follow_up = db.query_row(
            &format!(
                "SELECT COUNT(DISTINCT m.thread_id) FROM messages m JOIN folders f ON f.id=m.folder_id \
                 WHERE f.role='sent' AND {}",
                follow_up_cond("m")
            ),
            [],
            |r| r.get(0),
        )?;
        Ok(AiCounts { needs_reply, follow_up })
    }

    // ---------- follow-ups (feature 13) ----------

    /// Sent mail dated between `newer_than_ms` and `older_than_ms` that nobody answered and that was
    /// not checked yet. Only the newest sent message of a conversation, newest first.
    pub fn ai_followup_candidates(
        &self,
        allowed: &[String],
        newer_than_ms: i64,
        older_than_ms: i64,
        limit: usize,
    ) -> CoreResult<Vec<AiMessage>> {
        self.messages_where(
            &format!(
                "f.role='sent' AND m.draft=0 AND m.to_json<>'[]' AND m.date_ms>={newer_than_ms} \
                 AND m.date_ms<={older_than_ms} AND {} \
                 AND NOT EXISTS (SELECT 1 FROM ai_followups fu WHERE fu.message_id=m.id) \
                 AND NOT EXISTS (SELECT 1 FROM messages s JOIN folders fs ON fs.id=s.folder_id \
                     WHERE s.thread_id=m.thread_id AND fs.role='sent' AND s.date_ms>m.date_ms) \
                 AND NOT {} ORDER BY m.date_ms DESC",
                account_in("m", allowed),
                answered_by_them("m")
            ),
            limit,
        )
    }

    pub fn ai_save_followup(&self, message_id: i64, expects: bool, awaiting: &str, now_ms: i64) -> CoreResult<()> {
        self.db.lock().execute(
            "INSERT INTO ai_followups(message_id,expects,awaiting,checked_ms,notified,dismissed) VALUES(?1,?2,?3,?4,0,0)
             ON CONFLICT(message_id) DO UPDATE SET expects=excluded.expects, awaiting=excluded.awaiting,
               checked_ms=excluded.checked_ms",
            params![message_id, expects, awaiting, now_ms],
        )?;
        Ok(())
    }

    /// Follow-ups to tell the user about (once): expect an answer, got none, not dismissed.
    pub fn ai_followups_to_notify(&self) -> CoreResult<Vec<AiFollowUp>> {
        let db = self.db.lock();
        let mut st = db.prepare(&format!(
            "SELECT m.id, COALESCE(m.thread_id,0), m.subject, fu.awaiting FROM ai_followups fu \
             JOIN messages m ON m.id=fu.message_id WHERE fu.notified=0 AND {} ORDER BY m.date_ms",
            follow_up_cond("m")
        ))?;
        let rows = st
            .query_map([], |r| {
                Ok(AiFollowUp { message_id: r.get(0)?, thread_id: r.get(1)?, subject: r.get(2)?, awaiting: r.get(3)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn ai_mark_notified(&self, message_id: i64) -> CoreResult<()> {
        self.db.lock().execute("UPDATE ai_followups SET notified=1 WHERE message_id=?1", [message_id])?;
        Ok(())
    }

    // ---------- briefing (feature 15) ----------

    /// Unread inbox mail for the briefing, people first, then newest first.
    pub fn ai_unread_inbox(&self, allowed: &[String], limit: usize) -> CoreResult<Vec<AiMessage>> {
        // ORDER BY comes with the WHERE text; LIMIT is appended by messages_where
        self.messages_where(
            &format!(
                "f.role='inbox' AND m.seen=0 AND m.draft=0 AND t.done=0 AND t.snoozed_until IS NULL AND {} \
                 AND lower(m.from_addr) NOT IN (SELECT lower(email) FROM accounts) \
                 ORDER BY (m.category='personal') DESC, m.date_ms DESC",
                account_in("m", allowed)
            ),
            limit,
        )
    }

    pub fn ai_briefing(&self, day: &str) -> CoreResult<Option<(String, i64)>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT markdown, at_ms FROM ai_briefings WHERE day=?1", [day], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?)
    }

    /// The newest stored briefing: `(day, markdown, at_ms)`.
    pub fn ai_latest_briefing(&self) -> CoreResult<Option<(String, String, i64)>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT day, markdown, at_ms FROM ai_briefings ORDER BY day DESC LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
    }

    pub fn ai_save_briefing(&self, day: &str, markdown: &str, now_ms: i64) -> CoreResult<()> {
        let db = self.db.lock();
        db.execute(
            "INSERT INTO ai_briefings(day,markdown,at_ms) VALUES(?1,?2,?3)
             ON CONFLICT(day) DO UPDATE SET markdown=excluded.markdown, at_ms=excluded.at_ms",
            params![day, markdown, now_ms],
        )?;
        // a month of briefings is plenty
        db.execute(
            "DELETE FROM ai_briefings WHERE day < (SELECT day FROM ai_briefings ORDER BY day DESC LIMIT 1 OFFSET 30)",
            [],
        )?;
        Ok(())
    }

    /// The conversation a message belongs to.
    pub fn thread_of_message(&self, message_id: i64) -> CoreResult<Option<i64>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT thread_id FROM messages WHERE id=?1", [message_id], |r| r.get::<_, Option<i64>>(0))
            .optional()?
            .flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Account, ListFilter, NewMessage};
    use serde_json::json;

    const ME: &str = "me@x.org";
    const DAY: i64 = 86_400_000;

    struct Fx {
        store: Store,
        inbox: i64,
        sent: i64,
        uid: u32,
    }

    fn fx() -> Fx {
        let store = Store::in_memory().unwrap();
        store
            .upsert_account(&Account {
                email: ME.into(),
                account_type: "customIMAP".into(),
                imap_host: "h".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                smtp_host: "h".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                username: ME.into(),
                ..Default::default()
            })
            .unwrap();
        let inbox = store.upsert_folder(ME, "INBOX", "INBOX", Some("/"), Some("inbox"), true).unwrap();
        let sent = store.upsert_folder(ME, "Sent", "Sent", Some("/"), Some("sent"), true).unwrap();
        store.set_ai_labels_applied(true);
        Fx { store, inbox, sent, uid: 0 }
    }

    impl Fx {
        fn add(
            &mut self,
            folder: i64,
            from: &str,
            to: &str,
            subject: &str,
            date: i64,
            mid: &str,
            irt: Option<&str>,
        ) -> i64 {
            self.uid += 1;
            self.store
                .insert_message(&NewMessage {
                    account: ME.into(),
                    folder_id: folder,
                    uid: self.uid,
                    message_id: Some(mid.into()),
                    in_reply_to: irt.map(str::to_string),
                    subject: subject.into(),
                    from_addr: from.into(),
                    to: json!([{ "name": null, "mailbox": to }]),
                    cc: json!([]),
                    bcc: json!([]),
                    reply_to: json!([]),
                    date_ms: date,
                    preview: format!("preview of {subject}"),
                    body_text: Some(format!("body of {subject}")),
                    category: "personal".into(),
                    ..Default::default()
                })
                .unwrap()
        }
        fn rows(&self, f: ListFilter) -> Vec<crate::store::ThreadRow> {
            self.store.thread_rows(&f, 100).unwrap()
        }
        fn inbox_cat(&self, c: &str) -> Vec<i64> {
            self.rows(ListFilter::Inbox { account: None, done: false, category: Some(c.into()), with_done: false })
                .iter()
                .map(|r| r.thread_id)
                .collect()
        }
        fn label(&self, id: i64, cat: &str, needs: bool) {
            self.store
                .ai_save_labels(
                    &[AiLabelIn {
                        message_id: id,
                        category: cat.into(),
                        needs_reply: needs,
                        priority: 1,
                        reason: "because".into(),
                    }],
                    "m",
                    1,
                )
                .unwrap();
        }
    }

    #[test]
    fn category_override_applies_only_to_unsure_mail_and_is_reversible() {
        let mut f = fx();
        let stranger = f.add(f.inbox, "news@shop.example", ME, "Sale", 1000, "<a>", None);
        let friend = f.add(f.inbox, "maya@x.org", ME, "Hi", 2000, "<b>", None);
        f.add(f.sent, ME, "maya@x.org", "Re: Hi", 1500, "<c>", None); // the user wrote to Maya
        assert_eq!(f.inbox_cat("personal").len(), 2);
        f.label(stranger, "newsletters", false);
        f.label(friend, "newsletters", false);
        let l = f.store.ai_label(stranger).unwrap().unwrap();
        assert!(l.eligible && l.category == "newsletters");
        assert!(!f.store.ai_label(friend).unwrap().unwrap().eligible, "the user wrote to this sender");
        let nl = f.inbox_cat("newsletters");
        assert_eq!(nl.len(), 1, "only the stranger moved");
        assert_eq!(f.inbox_cat("personal").len(), 1);
        let c = f.store.sidebar_counts().unwrap();
        assert_eq!(c.categories["newsletters"].1, 1);
        assert_eq!(c.categories["personal"].1, 1);
        // the list rows carry the shown category
        let rows = f.rows(ListFilter::Inbox { account: None, done: false, category: None, with_done: false });
        assert_eq!(rows.iter().filter(|r| r.category == "newsletters").count(), 1);
        // feature off: the rule category is back, nothing was lost
        f.store.set_ai_labels_applied(false);
        assert_eq!(f.inbox_cat("personal").len(), 2);
        assert!(f.inbox_cat("newsletters").is_empty());
        assert_eq!(f.store.sidebar_counts().unwrap().categories["personal"].1, 2);
        f.store.set_ai_labels_applied(true);
        assert_eq!(f.inbox_cat("newsletters").len(), 1, "and on again");
    }

    #[test]
    fn rule_categories_are_not_overridden() {
        let mut f = fx();
        let id = f.add(f.inbox, "noreply@x.com", ME, "Receipt", 1000, "<a>", None);
        f.store.db.lock().execute("UPDATE messages SET category='notifications' WHERE id=?1", [id]).unwrap();
        f.label(id, "personal", true);
        assert!(!f.store.ai_label(id).unwrap().unwrap().eligible);
        assert_eq!(f.inbox_cat("notifications").len(), 1);
        assert!(f.inbox_cat("personal").is_empty());
    }

    #[test]
    fn needs_reply_list_flag_count_and_the_ways_it_ends() {
        let mut f = fx();
        let a = f.add(f.inbox, "maya@x.org", ME, "Plan?", 1000, "<a>", None);
        let b = f.add(f.inbox, "jonas@x.org", ME, "FYI", 2000, "<b>", None);
        let c = f.add(f.inbox, "lea@x.org", ME, "Contract?", 3000, "<c>", None);
        f.label(a, "personal", true);
        f.label(b, "personal", false);
        f.label(c, "personal", true);
        let list = f.rows(ListFilter::NeedsReply { account: None });
        assert_eq!(list.iter().map(|r| r.subject.as_str()).collect::<Vec<_>>(), vec!["Contract?", "Plan?"]);
        assert!(list.iter().all(|r| r.needs_reply && r.ai_reason == "because"));
        let inbox = f.rows(ListFilter::Inbox { account: None, done: false, category: None, with_done: false });
        assert_eq!(inbox.iter().filter(|r| r.needs_reply).count(), 2, "the flag is in every list");
        assert_eq!(f.store.ai_counts().unwrap().needs_reply, 2);
        assert_eq!(f.store.sidebar_counts().unwrap().needs_reply, 2);
        // the user replies in the thread: ends
        f.add(f.sent, ME, "maya@x.org", "Re: Plan?", 1500, "<r>", Some("<a>"));
        let after = f.rows(ListFilter::NeedsReply { account: None });
        assert_eq!(after.len(), 1);
        // Done ends it
        f.store.set_thread_flag(after[0].thread_id, "done", true).unwrap();
        assert!(f.rows(ListFilter::NeedsReply { account: None }).is_empty());
        assert_eq!(f.store.ai_counts().unwrap().needs_reply, 0);
        // "Not needed" ends it too
        let d = f.add(f.inbox, "sam@x.org", ME, "Lunch?", 4000, "<d>", None);
        f.label(d, "personal", true);
        let t = f.store.thread_of_message(d).unwrap().unwrap();
        assert_eq!(f.rows(ListFilter::NeedsReply { account: None }).len(), 1);
        f.store.ai_dismiss_thread(t).unwrap();
        assert!(f.rows(ListFilter::NeedsReply { account: None }).is_empty());
        // a new label for the same message does not bring it back
        f.label(d, "personal", true);
        assert!(f.rows(ListFilter::NeedsReply { account: None }).is_empty());
    }

    #[test]
    fn needs_reply_respects_account_filter_and_answered_flag() {
        let mut f = fx();
        let a = f.add(f.inbox, "maya@x.org", ME, "Plan?", 1000, "<a>", None);
        f.label(a, "personal", true);
        assert_eq!(f.rows(ListFilter::NeedsReply { account: Some(ME.into()) }).len(), 1);
        assert!(f.rows(ListFilter::NeedsReply { account: Some("other@x.org".into()) }).is_empty());
        f.store.set_answered(a).unwrap();
        assert!(f.rows(ListFilter::NeedsReply { account: None }).is_empty(), "answered on the server");
    }

    #[test]
    fn follow_ups_candidates_list_and_the_ways_they_end() {
        let mut f = fx();
        let now = 100 * DAY;
        let old = f.add(f.sent, ME, "lea@y.org", "Can you sign?", now - 5 * DAY, "<s1>", None);
        let fresh = f.add(f.sent, ME, "lea@y.org", "Fresh", now - DAY, "<s2>", None);
        let answered = f.add(f.sent, ME, "bob@y.org", "Question", now - 6 * DAY, "<s3>", None);
        f.add(f.inbox, "bob@y.org", ME, "Re: Question", now - 4 * DAY, "<i3>", Some("<s3>"));
        let ancient = f.add(f.sent, ME, "lea@y.org", "Ancient", now - 60 * DAY, "<s4>", None);
        let allowed = vec![ME.to_string()];
        let c = f.store.ai_followup_candidates(&allowed, now - 30 * DAY, now - 3 * DAY, 10).unwrap();
        let ids: Vec<i64> = c.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![old], "not fresh, not answered, not older than 30 days");
        assert!(!ids.contains(&fresh) && !ids.contains(&answered) && !ids.contains(&ancient));
        assert!(f.store.ai_followup_candidates(&[], 0, now, 10).unwrap().is_empty(), "no allowed account");
        assert!(f.store.ai_followup_candidates(&["x@y.z".into()], 0, now, 10).unwrap().is_empty());
        // checked: cached, so no longer a candidate; the list shows it with what is awaited
        f.store.ai_save_followup(old, true, "signed contract", now).unwrap();
        assert!(f.store.ai_followup_candidates(&allowed, now - 30 * DAY, now - 3 * DAY, 10).unwrap().is_empty());
        let list = f.rows(ListFilter::FollowUp { account: None });
        assert_eq!(list.len(), 1);
        assert!(list[0].follow_up && list[0].awaiting == "signed contract" && list[0].subject == "Can you sign?");
        assert_eq!(f.store.ai_counts().unwrap().follow_up, 1);
        assert_eq!(f.store.sidebar_counts().unwrap().follow_up, 1);
        // notified once
        let n = f.store.ai_followups_to_notify().unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].awaiting, "signed contract");
        f.store.ai_mark_notified(old).unwrap();
        assert!(f.store.ai_followups_to_notify().unwrap().is_empty());
        // the answer arrives: the follow-up ends
        f.add(f.inbox, "lea@y.org", ME, "Re: Can you sign?", now - 2 * DAY, "<i1>", Some("<s1>"));
        assert!(f.rows(ListFilter::FollowUp { account: None }).is_empty());
        assert_eq!(f.store.ai_counts().unwrap().follow_up, 0);
    }

    #[test]
    fn follow_up_not_expected_or_dismissed_is_not_listed() {
        let mut f = fx();
        let thanks = f.add(f.sent, ME, "lea@y.org", "Thanks!", 1000, "<s1>", None);
        let ask = f.add(f.sent, ME, "lea@y.org", "Ask", 2000, "<s2>", None);
        f.store.ai_save_followup(thanks, false, "", 5000).unwrap();
        f.store.ai_save_followup(ask, true, "answer", 5000).unwrap();
        assert_eq!(f.rows(ListFilter::FollowUp { account: None }).len(), 1);
        let t = f.store.thread_of_message(ask).unwrap().unwrap();
        f.store.ai_dismiss_thread(t).unwrap();
        assert!(f.rows(ListFilter::FollowUp { account: None }).is_empty());
        assert!(f.store.ai_followups_to_notify().unwrap().is_empty());
        // only the newest sent message of a conversation is checked
        let first = f.add(f.sent, ME, "z@y.org", "Q1", 3000, "<q1>", None);
        f.add(f.sent, ME, "z@y.org", "Re: Q1", 4000, "<q2>", Some("<q1>"));
        let c = f.store.ai_followup_candidates(&[ME.into()], 0, 10_000, 10).unwrap();
        assert!(!c.iter().any(|m| m.id == first));
    }

    #[test]
    fn unlabeled_mail_respects_the_switch_on_time_accounts_and_rules() {
        let mut f = fx();
        let old = f.add(f.inbox, "a@x.org", ME, "old", 1000, "<o>", None);
        let new = f.add(f.inbox, "b@x.org", ME, "new", 9000, "<n>", None);
        let inv = f.add(f.inbox, "c@x.org", ME, "inv", 9100, "<i>", None);
        f.store.db.lock().execute("UPDATE messages SET category='invitations' WHERE id=?1", [inv]).unwrap();
        let bulk = f.add(f.inbox, "d@x.org", ME, "bulk", 9200, "<b>", None);
        f.store.db.lock().execute("UPDATE messages SET category='newsletters' WHERE id=?1", [bulk]).unwrap();
        f.add(f.inbox, ME, ME, "from me", 9300, "<m>", None);
        let allowed = vec![ME.to_string()];
        let ids = |m: Vec<AiMessage>| m.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(f.store.ai_unlabeled(&allowed, 5000, false, 20).unwrap()),
            vec![new],
            "older mail is never sent"
        );
        assert_eq!(ids(f.store.ai_unlabeled(&allowed, 5000, true, 20).unwrap()), vec![inv, new]);
        assert_eq!(ids(f.store.ai_unlabeled(&allowed, 0, false, 20).unwrap()), vec![new, old]);
        assert!(f.store.ai_unlabeled(&[], 0, true, 20).unwrap().is_empty(), "excluded account");
        f.label(new, "personal", false);
        f.store.ai_save_skipped(&[old, new], 1).unwrap();
        assert!(
            f.store.ai_unlabeled(&allowed, 0, false, 20).unwrap().is_empty(),
            "labelled or skipped: not asked again"
        );
        let skipped = f.store.ai_label(old).unwrap().unwrap();
        assert_eq!((skipped.model.as_str(), skipped.eligible, skipped.needs_reply), ("skipped", false, false));
        assert_eq!(f.store.ai_label(new).unwrap().unwrap().model, "m", "a real label is not replaced by skipped");
    }

    #[test]
    fn briefing_input_is_unread_people_first_and_stored_per_day() {
        let mut f = fx();
        let read = f.add(f.inbox, "a@x.org", ME, "read", 1000, "<r>", None);
        f.store.set_flags(f.inbox, 1, true, false).unwrap();
        let n = f.add(f.inbox, "ci@x.org", ME, "build", 9000, "<n>", None);
        f.store.db.lock().execute("UPDATE messages SET category='notifications' WHERE id=?1", [n]).unwrap();
        let p = f.add(f.inbox, "maya@x.org", ME, "plan", 5000, "<p>", None);
        let u = f.store.ai_unread_inbox(&[ME.into()], 40).unwrap();
        assert_eq!(u.iter().map(|m| m.id).collect::<Vec<_>>(), vec![p, n], "people first, read mail left out");
        assert!(!u.iter().any(|m| m.id == read));
        assert_eq!(u[0].body, "body of plan");
        assert_eq!(u[0].to, ME);
        assert_eq!(f.store.ai_unread_inbox(&[ME.into()], 1).unwrap().len(), 1);
        assert!(f.store.ai_briefing("2026-10-29").unwrap().is_none());
        f.store.ai_save_briefing("2026-10-29", "## People\n- x", 77).unwrap();
        f.store.ai_save_briefing("2026-10-29", "## People\n- y", 78).unwrap();
        f.store.ai_save_briefing("2026-10-28", "old", 70).unwrap();
        assert_eq!(f.store.ai_briefing("2026-10-29").unwrap(), Some(("## People\n- y".to_string(), 78)));
        assert_eq!(f.store.ai_latest_briefing().unwrap().unwrap().0, "2026-10-29");
    }

    #[test]
    fn runner_state_round_trips() {
        let f = fx();
        assert_eq!(f.store.ai_state("k"), None);
        f.store.ai_set_state("k", "1").unwrap();
        f.store.ai_set_state("k", "2").unwrap();
        assert_eq!(f.store.ai_state("k").as_deref(), Some("2"));
        f.store.ai_clear_state("k").unwrap();
        assert_eq!(f.store.ai_state("k"), None);
    }

    #[test]
    fn labels_vanish_with_their_message() {
        let mut f = fx();
        let a = f.add(f.inbox, "maya@x.org", ME, "Plan?", 1000, "<a>", None);
        f.label(a, "personal", true);
        f.store.delete_uids(f.inbox, &[1]).unwrap();
        assert!(f.store.ai_label(a).unwrap().is_none());
    }
}
