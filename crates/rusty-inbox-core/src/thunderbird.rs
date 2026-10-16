//! Import accounts from a Thunderbird profile: servers and identities from `prefs.js`, saved
//! passwords from `logins.json` decrypted with the profile's `key4.db` (works when the profile
//! has no primary password, which is Thunderbird's default).
//!
//! Profiles are found in the standard places for each OS, including the Flatpak and Snap
//! Thunderbird on Linux.

use base64::Engine as _;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One importable account.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TbAccount {
    pub email: String,
    pub name: String,
    pub username: String,
    pub imap_host: String,
    pub imap_port: u16,
    /// tls | starttls | none
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    /// Thunderbird signs in with OAuth (Gmail, Outlook): no stored password to bring over
    pub oauth: bool,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    pub has_password: bool,
    pub profile: String,
}

/// Profile directories of every Thunderbird installation found for this user.
pub fn profiles() -> Vec<PathBuf> {
    let home =
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let mut roots = vec![
        home.join(".thunderbird"),
        home.join(".var/app/org.mozilla.thunderbird/.thunderbird"),
        home.join("snap/thunderbird/common/.thunderbird"),
        home.join("Library/Thunderbird"),
    ];
    if let Some(a) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(a).join("Thunderbird"));
    }
    let mut out = vec![];
    for root in roots {
        let Ok(ini) = std::fs::read_to_string(root.join("profiles.ini")) else { continue };
        for p in profile_paths(&ini) {
            let (rel, path) = p;
            let dir = if rel { root.join(&path) } else { PathBuf::from(&path) };
            if dir.join("prefs.js").exists() && !out.contains(&dir) {
                out.push(dir);
            }
        }
    }
    out
}

/// `(is_relative, path)` of each `[ProfileN]` in profiles.ini.
pub fn profile_paths(ini: &str) -> Vec<(bool, String)> {
    let mut out = vec![];
    let mut in_profile = false;
    let mut rel = true;
    let mut path: Option<String> = None;
    let mut flush = |rel: bool, path: &mut Option<String>| {
        if let Some(p) = path.take() {
            out.push((rel, p));
        }
    };
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') {
            if in_profile {
                flush(rel, &mut path);
            }
            in_profile = line.starts_with("[Profile");
            rel = true;
        } else if in_profile {
            if let Some(v) = line.strip_prefix("Path=") {
                path = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("IsRelative=") {
                rel = v != "0";
            }
        }
    }
    if in_profile {
        flush(rel, &mut path);
    }
    out
}

/// `user_pref("key", value);` lines of prefs.js (strings unquoted and unescaped).
pub fn parse_prefs(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("user_pref(\"") else { continue };
        let Some((key, rest)) = rest.split_once("\",") else { continue };
        let v = rest.trim().trim_end_matches(';').trim().trim_end_matches(')').trim();
        let v = match v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            Some(s) => s.replace("\\\"", "\"").replace("\\\\", "\\"),
            None => v.to_string(),
        };
        m.insert(key.to_string(), v);
    }
    m
}

fn security(socket_type: Option<&String>) -> &'static str {
    // Thunderbird socketType: 0 plain, 2 STARTTLS, 3 SSL/TLS
    match socket_type.map(String::as_str) {
        Some("3") => "tls",
        Some("2") => "starttls",
        Some("0") => "none",
        _ => "tls",
    }
}

/// IMAP accounts described by a profile's prefs (passwords not filled in).
pub fn accounts_from_prefs(prefs: &HashMap<String, String>, profile: &str) -> Vec<TbAccount> {
    let get = |k: String| prefs.get(&k).cloned().unwrap_or_default();
    let list = get("mail.accountmanager.accounts".into());
    let mut out = vec![];
    for acct in list.split(',').map(str::trim).filter(|a| !a.is_empty()) {
        let srv = get(format!("mail.account.{acct}.server"));
        if srv.is_empty() || get(format!("mail.server.{srv}.type")) != "imap" {
            continue;
        }
        let ident = get(format!("mail.account.{acct}.identities"));
        let ident = ident.split(',').next().unwrap_or("").trim().to_string();
        let email = get(format!("mail.identity.{ident}.useremail")).trim().to_lowercase();
        let username = get(format!("mail.server.{srv}.userName"));
        let email = if email.is_empty() { username.to_lowercase() } else { email };
        if !email.contains('@') {
            continue;
        }
        let imap_security = security(prefs.get(&format!("mail.server.{srv}.socketType"))).to_string();
        let imap_port =
            get(format!("mail.server.{srv}.port")).parse().unwrap_or(if imap_security == "tls" { 993 } else { 143 });
        let smtp = get(format!("mail.identity.{ident}.smtpServer"));
        let smtp = if smtp.is_empty() { get("mail.smtp.defaultserver".into()) } else { smtp };
        let smtp_security = security(prefs.get(&format!("mail.smtpserver.{smtp}.try_ssl"))).to_string();
        let smtp_host = get(format!("mail.smtpserver.{smtp}.hostname"));
        let imap_host = get(format!("mail.server.{srv}.hostname"));
        // authMethod 10 = OAuth2
        let oauth = get(format!("mail.server.{srv}.authMethod")) == "10";
        out.push(TbAccount {
            name: get(format!("mail.identity.{ident}.fullName")),
            username: if username.is_empty() { email.clone() } else { username },
            smtp_port: get(format!("mail.smtpserver.{smtp}.port")).parse().unwrap_or(if smtp_security == "tls" {
                465
            } else {
                587
            }),
            smtp_host: if smtp_host.is_empty() { imap_host.clone() } else { smtp_host },
            imap_host,
            imap_port,
            imap_security,
            smtp_security,
            oauth,
            password: None,
            has_password: false,
            profile: profile.to_string(),
            email,
        });
    }
    out
}

/// Every IMAP account of every Thunderbird profile, with its saved password when it can be read.
pub fn find_accounts() -> Vec<TbAccount> {
    let mut out: Vec<TbAccount> = vec![];
    for dir in profiles() {
        let Ok(text) = std::fs::read_to_string(dir.join("prefs.js")) else { continue };
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut accts = accounts_from_prefs(&parse_prefs(&text), &name);
        let logins = saved_logins(&dir).unwrap_or_else(|e| {
            tracing::info!("thunderbird passwords in {name}: {e:#}");
            vec![]
        });
        for a in &mut accts {
            let host = format!("imap://{}", a.imap_host);
            a.password = logins
                .iter()
                .find(|(h, u, _)| h.eq_ignore_ascii_case(&host) && u.eq_ignore_ascii_case(&a.username))
                .map(|(_, _, p)| p.clone());
            a.has_password = a.password.is_some();
        }
        for a in accts {
            if !out.iter().any(|x| x.email == a.email) {
                out.push(a);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// saved passwords (NSS key4.db + logins.json)
// ---------------------------------------------------------------------------------------------

/// `(hostname, username, password)` of every saved login of a profile.
pub fn saved_logins(profile: &Path) -> anyhow::Result<Vec<(String, String, String)>> {
    let keys = master_keys(profile)?;
    let text = std::fs::read_to_string(profile.join("logins.json"))?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let mut out = vec![];
    for l in v["logins"].as_array().cloned().unwrap_or_default() {
        let host = l["hostname"].as_str().unwrap_or("").to_string();
        let dec = |k: &str| -> Option<String> {
            let raw = base64::engine::general_purpose::STANDARD.decode(l[k].as_str()?).ok()?;
            String::from_utf8(decrypt_login_field(&keys, &raw).ok()?).ok()
        };
        if let (Some(u), Some(p)) = (dec("encryptedUsername"), dec("encryptedPassword")) {
            out.push((host, u, p));
        }
    }
    Ok(out)
}

/// The profile's login-encryption keys `(key id, key)`, unlocked with an empty primary password.
fn master_keys(profile: &Path) -> anyhow::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    // read a copy: a running Thunderbird keeps the database open
    let tmp = std::env::temp_dir().join(format!("rusty-inbox-key4-{}.db", std::process::id()));
    std::fs::copy(profile.join("key4.db"), &tmp)?;
    let res = (|| -> anyhow::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let db = rusqlite::Connection::open_with_flags(&tmp, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let (salt, check): (Vec<u8>, Vec<u8>) =
            db.query_row("SELECT item1, item2 FROM metaData WHERE id = 'password'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let clear = pbe_decrypt(&check, &salt, b"")?;
        if !clear.starts_with(b"password-check") {
            anyhow::bail!("the profile is protected by a primary password");
        }
        // one row per key (older 3DES and newer AES-256); a102 is the id logins refer to
        let mut st = db.prepare("SELECT a102, a11 FROM nssPrivate")?;
        let rows: Vec<(Vec<u8>, Vec<u8>)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.filter_map(Result::ok).collect();
        let keys: Vec<(Vec<u8>, Vec<u8>)> =
            rows.into_iter().filter_map(|(id, a11)| pbe_decrypt(&a11, &salt, b"").ok().map(|k| (id, k))).collect();
        if keys.is_empty() {
            anyhow::bail!("no usable key in key4.db");
        }
        Ok(keys)
    })();
    let _ = std::fs::remove_file(&tmp);
    res
}

const OID_PBES2: &str = "1.2.840.113549.1.5.13";
const OID_DES3: &str = "1.2.840.113549.3.7";
const OID_AES256: &str = "2.16.840.1.101.3.4.1.42";

/// Decrypt an NSS PBES2 blob (PBKDF2-HMAC-SHA256 + AES-256-CBC, keyed by
/// SHA1(global salt + primary password)).
fn pbe_decrypt(der: &[u8], global_salt: &[u8], primary_password: &[u8]) -> anyhow::Result<Vec<u8>> {
    use sha1::Digest;
    let top = Der::parse(der)?;
    let algo = top.child(0)?;
    let oid = algo.child(0)?.oid()?;
    if oid != OID_PBES2 {
        anyhow::bail!("unsupported key encryption {oid} (very old profile)");
    }
    let params = algo.child(1)?;
    let kdf = params.child(0)?.child(1)?;
    let entry_salt = kdf.child(0)?.content;
    let iterations = kdf.child(1)?.uint()? as u32;
    let key_len = kdf.child(2)?.uint()? as usize;
    let iv_tail = params.child(1)?.child(1)?.content;
    let cipher = top.child(1)?.content;

    let mut h = sha1::Sha1::new();
    h.update(global_salt);
    h.update(primary_password);
    let k = h.finalize();
    let mut key = vec![0u8; key_len];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(&k, entry_salt, iterations, &mut key);
    // NSS stores 14 bytes of the IV; the full IV is the DER header 04 0e plus those bytes
    let mut iv = vec![0x04, 0x0e];
    iv.extend_from_slice(iv_tail);
    aes256_cbc(&key, &iv, cipher)
}

/// Decrypt one `encryptedUsername` / `encryptedPassword` value. The key is the one with the
/// field's key id and the cipher's size (a profile can hold a 3DES and an AES key under one id).
fn decrypt_login_field(keys: &[(Vec<u8>, Vec<u8>)], der: &[u8]) -> anyhow::Result<Vec<u8>> {
    let top = Der::parse(der)?;
    let key_id = top.child(0)?.content;
    let algo = top.child(1)?;
    let oid = algo.child(0)?.oid()?;
    let iv = algo.child(1)?.content;
    let data = top.child(2)?.content;
    let size = match oid.as_str() {
        OID_DES3 => 24,
        OID_AES256 => 32,
        o => anyhow::bail!("unsupported login cipher {o}"),
    };
    let pick = |same_id: bool| {
        keys.iter().find(|(id, k)| k.len() == size && (!same_id || id.as_slice() == key_id)).map(|(_, k)| k.as_slice())
    };
    let key = pick(true).or_else(|| pick(false)).ok_or_else(|| anyhow::anyhow!("no {size}-byte key"))?;
    if size == 24 {
        des3_cbc(key, iv, data)
    } else {
        aes256_cbc(key, iv, data)
    }
}

fn aes256_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|e| anyhow::anyhow!("decrypt: {e}"))
}

fn des3_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    cbc::Decryptor::<des::TdesEde3>::new_from_slices(key, iv)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|e| anyhow::anyhow!("decrypt: {e}"))
}

/// Just enough DER to walk NSS structures.
struct Der<'a> {
    tag: u8,
    content: &'a [u8],
}

impl<'a> Der<'a> {
    fn parse(b: &'a [u8]) -> anyhow::Result<Der<'a>> {
        Ok(Self::read(b)?.0)
    }

    fn read(b: &'a [u8]) -> anyhow::Result<(Der<'a>, &'a [u8])> {
        let short = || anyhow::anyhow!("truncated DER");
        let tag = *b.first().ok_or_else(short)?;
        let l0 = *b.get(1).ok_or_else(short)? as usize;
        let (len, hdr) = if l0 < 0x80 {
            (l0, 2)
        } else {
            let n = l0 & 0x7f;
            if n == 0 || n > 4 {
                anyhow::bail!("bad DER length");
            }
            let mut len = 0usize;
            for i in 0..n {
                len = (len << 8) | *b.get(2 + i).ok_or_else(short)? as usize;
            }
            (len, 2 + n)
        };
        let content = b.get(hdr..hdr + len).ok_or_else(short)?;
        Ok((Der { tag, content }, &b[hdr + len..]))
    }

    fn children(&self) -> anyhow::Result<Vec<Der<'a>>> {
        let mut out = vec![];
        let mut rest = self.content;
        while !rest.is_empty() {
            let (d, r) = Self::read(rest)?;
            out.push(d);
            rest = r;
        }
        Ok(out)
    }

    fn child(&self, i: usize) -> anyhow::Result<Der<'a>> {
        self.children()?.into_iter().nth(i).ok_or_else(|| anyhow::anyhow!("missing DER element {i}"))
    }

    fn uint(&self) -> anyhow::Result<u64> {
        if self.tag != 0x02 || self.content.len() > 8 {
            anyhow::bail!("not a small INTEGER");
        }
        Ok(self.content.iter().fold(0u64, |a, b| (a << 8) | *b as u64))
    }

    fn oid(&self) -> anyhow::Result<String> {
        if self.tag != 0x06 || self.content.is_empty() {
            anyhow::bail!("not an OID");
        }
        let first = self.content[0];
        let mut parts = vec![(first / 40) as u64, (first % 40) as u64];
        let mut v = 0u64;
        for b in &self.content[1..] {
            v = (v << 7) | (b & 0x7f) as u64;
            if b & 0x80 == 0 {
                parts.push(v);
                v = 0;
            }
        }
        Ok(parts.iter().map(u64::to_string).collect::<Vec<_>>().join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFS: &str = r#"
user_pref("mail.accountmanager.accounts", "account1,account2,account4");
user_pref("mail.account.account1.server", "server1");
user_pref("mail.account.account1.identities", "id1");
user_pref("mail.server.server1.type", "imap");
user_pref("mail.server.server1.hostname", "mail.example.com");
user_pref("mail.server.server1.port", 993);
user_pref("mail.server.server1.socketType", 3);
user_pref("mail.server.server1.userName", "ann@example.com");
user_pref("mail.identity.id1.useremail", "Ann@Example.com");
user_pref("mail.identity.id1.fullName", "Ann \"A\" Lee");
user_pref("mail.identity.id1.smtpServer", "smtp1");
user_pref("mail.smtpserver.smtp1.hostname", "smtp.example.com");
user_pref("mail.smtpserver.smtp1.port", 587);
user_pref("mail.smtpserver.smtp1.try_ssl", 2);
user_pref("mail.account.account2.server", "server2");
user_pref("mail.server.server2.type", "none");
user_pref("mail.account.account4.server", "server4");
user_pref("mail.account.account4.identities", "id4");
user_pref("mail.server.server4.type", "imap");
user_pref("mail.server.server4.hostname", "imap.gmail.com");
user_pref("mail.server.server4.authMethod", 10);
user_pref("mail.server.server4.userName", "bob@gmail.com");
user_pref("mail.identity.id4.useremail", "bob@gmail.com");
"#;

    #[test]
    fn accounts_come_from_prefs() {
        let a = accounts_from_prefs(&parse_prefs(PREFS), "p");
        assert_eq!(a.len(), 2, "Local Folders skipped");
        assert_eq!(a[0].email, "ann@example.com");
        assert_eq!(a[0].name, "Ann \"A\" Lee");
        assert_eq!(
            (a[0].imap_host.as_str(), a[0].imap_port, a[0].imap_security.as_str()),
            ("mail.example.com", 993, "tls")
        );
        assert_eq!(
            (a[0].smtp_host.as_str(), a[0].smtp_port, a[0].smtp_security.as_str()),
            ("smtp.example.com", 587, "starttls")
        );
        assert!(!a[0].oauth);
        assert!(a[1].oauth);
        assert_eq!(a[1].smtp_host, "imap.gmail.com", "falls back to the IMAP host");
    }

    #[test]
    fn profiles_ini_paths() {
        let ini = "[General]\nStartWithLastProfile=1\n\n[Profile1]\nName=b\nIsRelative=0\nPath=/abs/b\n\n[Profile0]\nName=a\nIsRelative=1\nPath=a.default\n\n[InstallX]\nDefault=a.default\n";
        assert_eq!(profile_paths(ini), vec![(false, "/abs/b".to_string()), (true, "a.default".to_string())]);
    }

    #[test]
    fn der_and_ciphers_roundtrip() {
        use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        assert!(Der::parse(&[0x06, 0x09, 0x2a]).is_err(), "truncated");
        let oid = Der::parse(&[0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x03, 0x07]).unwrap().oid().unwrap();
        assert_eq!(oid, OID_DES3);
        // an AES login field the way NSS writes it
        let key = [7u8; 32];
        let iv = [9u8; 16];
        let ct = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(b"secret");
        let aes_oid = [0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2a];
        let mut algo = aes_oid.to_vec();
        algo.extend([0x04, 16]);
        algo.extend(iv);
        let mut body = vec![0x04, 0x01, 0x00, 0x30, algo.len() as u8];
        body.extend(&algo);
        body.extend([0x04, ct.len() as u8]);
        body.extend(&ct);
        let mut der = vec![0x30, body.len() as u8];
        der.extend(body);
        // same id for a 3DES and an AES key, as real profiles have
        let keys = vec![(vec![0u8], vec![3u8; 24]), (vec![0u8], key.to_vec())];
        assert_eq!(decrypt_login_field(&keys, &der).unwrap(), b"secret", "picks the AES-sized key");
    }
}
