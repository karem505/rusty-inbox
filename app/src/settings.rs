//! App preferences shown in the Settings window. Each key has a typed default; values are stored
//! in the engine's settings table under `rustyInbox.<key>` and validated before they are saved.

use rusty_inbox_core::store::Store;
use serde_json::{json, Map, Value};

const PREFIX: &str = "rustyInbox.";

/// Every preference with its default value. The type of the default is the type a value must have.
pub fn defaults() -> Value {
    json!({
        // general
        "smartInbox": true,          // group the inbox into People / Notifications / Newsletters ...
        "collapseAutomated": true,   // Notifications and Newsletters start collapsed
        "markReadOnOpen": true,      // opening a conversation marks it read
        "undoSeconds": 5,            // how long Done / Delete / Snooze can be undone
        "syncMinutes": 1,            // how often to check for new mail
        "closeToTray": true,         // the close button keeps the app in the status bar
        "showDoneInInbox": false,    // keep Done conversations in the inbox, marked with a check
        // appearance
        "themeMode": "system",       // follow the system theme, or force light / dark
        "density": "comfortable",    // row height of the conversation list
        "showAvatars": true,
        "showPreview": true,         // second line of text under the subject
        // notifications
        "notifications": true,
        "notificationPreview": true, // show subject and text; off = only the sender
        "trayUnreadDot": true,       // unread dot on the status-bar icon
        // updates
        "autoUpdateCheck": true,     // look for a new release in the background
        "updateCheckDays": 1,        // how often: every day or every week
        "autoInstallUpdates": false, // download and apply it too (AppImage / binary installs)
    })
}

/// Allowed values for keys that are a choice rather than a free value.
pub fn choices(key: &str) -> Option<Vec<Value>> {
    let v = |xs: &[Value]| Some(xs.to_vec());
    match key {
        "undoSeconds" => v(&[json!(5), json!(10), json!(20), json!(30)]),
        "syncMinutes" => v(&[json!(1), json!(2), json!(5), json!(15), json!(30)]),
        "themeMode" => v(&[json!("system"), json!("light"), json!("dark")]),
        "density" => v(&[json!("comfortable"), json!("compact")]),
        "updateCheckDays" => v(&[json!(1), json!(7)]),
        _ => None,
    }
}

/// Check a value for `key`: the key must exist, the type must match the default, and choice keys
/// must use one of their choices.
pub fn validate(key: &str, value: &Value) -> Result<Value, String> {
    let d = defaults();
    let Some(default) = d.get(key) else {
        return Err(format!("unknown setting {key}"));
    };
    let same_type = matches!(
        (default, value),
        (Value::Bool(_), Value::Bool(_)) | (Value::Number(_), Value::Number(_)) | (Value::String(_), Value::String(_))
    );
    if !same_type {
        return Err(format!("{key}: wrong type"));
    }
    if let Some(c) = choices(key) {
        if !c.contains(value) {
            return Err(format!("{key}: {value} is not one of the choices"));
        }
    }
    Ok(value.clone())
}

/// Current preferences: defaults overlaid with whatever is stored (invalid stored values are ignored).
pub fn all(store: &Store) -> Value {
    let mut out = Map::new();
    if let Value::Object(d) = defaults() {
        for (k, default) in d {
            let stored = store.setting(&format!("{PREFIX}{k}")).ok().flatten();
            let v = stored.filter(|v| validate(&k, v).is_ok()).unwrap_or(default);
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

pub fn get(store: &Store, key: &str) -> Value {
    all(store)[key].clone()
}

/// Validate and save one preference.
pub fn set(store: &Store, key: &str, value: &Value) -> Result<(), String> {
    let v = validate(key, value)?;
    store.set_setting(&format!("{PREFIX}{key}"), &v).map_err(|e| e.to_string())
}

/// Saved message templates: `[{id, name, subject, html}]`, newest first.
pub fn templates(store: &Store) -> Value {
    match store.setting(&format!("{PREFIX}templates")).ok().flatten() {
        Some(Value::Array(a)) => Value::Array(a),
        _ => json!([]),
    }
}

/// Add a template (`id` 0 or missing) or replace the one with the same id. Returns its id.
pub fn save_template(store: &Store, t: &Value) -> Result<i64, String> {
    let name = t["name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err("Give the template a name".into());
    }
    let mut list = templates(store).as_array().cloned().unwrap_or_default();
    let mut id = t["id"].as_i64().unwrap_or(0);
    if id <= 0 {
        id = list.iter().filter_map(|x| x["id"].as_i64()).max().unwrap_or(0) + 1;
    }
    let entry = json!({
        "id": id,
        "name": name,
        "subject": t["subject"].as_str().unwrap_or(""),
        "html": t["html"].as_str().unwrap_or(""),
    });
    match list.iter().position(|x| x["id"].as_i64() == Some(id)) {
        Some(i) => list[i] = entry,
        None => list.insert(0, entry),
    }
    store.set_setting(&format!("{PREFIX}templates"), &Value::Array(list)).map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn delete_template(store: &Store, id: i64) -> Result<(), String> {
    let list: Vec<Value> = templates(store)
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|x| x["id"].as_i64() != Some(id))
        .collect();
    store.set_setting(&format!("{PREFIX}templates"), &Value::Array(list)).map_err(|e| e.to_string())
}

/// The signature added to new mail from `email` (empty: none).
pub fn signature(store: &Store, email: &str) -> String {
    store
        .setting(&format!("{PREFIX}signature:{email}"))
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn set_signature(store: &Store, email: &str, text: &str) -> Result<(), String> {
    store
        .set_setting(&format!("{PREFIX}signature:{email}"), &Value::String(text.to_string()))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_choices_include_defaults() {
        let d = defaults();
        for (k, v) in d.as_object().unwrap() {
            assert_eq!(validate(k, v), Ok(v.clone()), "{k}");
        }
    }

    #[test]
    fn validation_rejects_bad_values() {
        assert!(validate("nope", &json!(true)).is_err());
        assert!(validate("smartInbox", &json!("yes")).is_err());
        assert!(validate("undoSeconds", &json!(7)).is_err());
        assert!(validate("themeMode", &json!("blue")).is_err());
        assert_eq!(validate("themeMode", &json!("dark")), Ok(json!("dark")));
    }

    #[test]
    fn templates_add_replace_delete() {
        let s = Store::in_memory().unwrap();
        assert_eq!(templates(&s), json!([]));
        assert!(save_template(&s, &json!({"name": " "})).is_err());
        let a =
            save_template(&s, &json!({"name": "Thanks", "subject": "Thank you", "html": "<p>Thanks!</p>"})).unwrap();
        let b = save_template(&s, &json!({"name": "Invoice", "html": "<p>Attached</p>"})).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(templates(&s)[0]["name"], "Invoice", "newest first");
        save_template(&s, &json!({"id": a, "name": "Thanks v2", "html": "<p>Many thanks</p>"})).unwrap();
        assert_eq!(templates(&s).as_array().unwrap().len(), 2);
        assert_eq!(templates(&s)[1]["name"], "Thanks v2");
        delete_template(&s, b).unwrap();
        assert_eq!(templates(&s).as_array().unwrap().len(), 1);
    }

    #[test]
    fn signatures_are_per_account() {
        let s = Store::in_memory().unwrap();
        assert_eq!(signature(&s, "a@example.com"), "");
        set_signature(&s, "a@example.com", "Ann\nExample").unwrap();
        assert_eq!(signature(&s, "a@example.com"), "Ann\nExample");
        assert_eq!(signature(&s, "b@example.com"), "");
    }

    #[test]
    fn stored_values_overlay_defaults() {
        let s = Store::in_memory().unwrap();
        assert_eq!(get(&s, "undoSeconds"), json!(5));
        set(&s, "undoSeconds", &json!(20)).unwrap();
        set(&s, "smartInbox", &json!(false)).unwrap();
        assert!(set(&s, "undoSeconds", &json!(3)).is_err());
        let a = all(&s);
        assert_eq!(a["undoSeconds"], json!(20));
        assert_eq!(a["smartInbox"], json!(false));
        assert_eq!(a["density"], json!("comfortable"));
        // a value written behind our back with the wrong type falls back to the default
        s.set_setting("rustyInbox.density", &json!(3)).unwrap();
        assert_eq!(get(&s, "density"), json!("comfortable"));
    }
}
