//! Passwords for the person's own calendar accounts.
//!
//! - Native: the OS credential store (macOS Keychain, Windows Credential Manager, Linux Secret
//!   Service) via the `keyring` crate, service name `CalendarCraft`, account = calendar account id.
//! - Standalone web: WebCrypto AES-GCM with a non-extractable key kept in IndexedDB; only the
//!   ciphertext and nonce sit in `localStorage`. This protects a copied storage dump or a casual
//!   look at the disk, not script running on this origin.
//! - Inside artcraftCloud (`?host=parent`): the server keeps them sealed (see artcraftCloud's
//!   `/api/calendar/remote/credentials/:account`). The password is not kept in the browser.
//!
//! On start, a leftover plaintext `calendarcraft.secrets` map is moved into the new store and
//! then deleted.

use std::collections::BTreeMap;

use crate::host::{self, KEY_SECRETS};

/// Service name in the OS credential store.
pub const SERVICE: &str = "CalendarCraft";

/// A calendar account id: letters, digits, `_` or `-`, 1–64 characters (e.g. `acc_a1b2`).
pub fn valid_account_id(id: &str) -> bool {
    let n = id.len();
    (1..=64).contains(&n)
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
}

/// What a stored `calendarcraft.secrets` value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretsFile {
    /// The old map: account id → password, in the clear.
    Plain(BTreeMap<String, String>),
    /// Versioned ciphertext (standalone web).
    Encrypted(EncryptedFile),
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncryptedFile {
    pub v: u8,
    #[serde(default)]
    pub alg: String,
    #[serde(default)]
    pub accounts: BTreeMap<String, EncryptedEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncryptedEntry {
    /// Base64 AES-GCM nonce (12 bytes).
    pub n: String,
    /// Base64 ciphertext and tag.
    pub c: String,
}

/// Parse a `calendarcraft.secrets` value. A JSON object of strings is the old plaintext map;
/// `{ "v": 1, "accounts": { … } }` is the encrypted form.
pub fn parse_secrets_file(text: &str) -> SecretsFile {
    let text = text.trim();
    if text.is_empty() {
        return SecretsFile::Empty;
    }
    if let Ok(enc) = serde_json::from_str::<EncryptedFile>(text) {
        if enc.v >= 1 {
            return SecretsFile::Encrypted(enc);
        }
    }
    if let Ok(map) = serde_json::from_str::<BTreeMap<String, String>>(text) {
        if map.is_empty() {
            return SecretsFile::Empty;
        }
        return SecretsFile::Plain(map);
    }
    SecretsFile::Empty
}

/// True when `text` is the old plaintext map (so it must be migrated).
pub fn is_plaintext(text: &str) -> bool {
    matches!(parse_secrets_file(text), SecretsFile::Plain(_))
}

/// Take each plaintext entry, store it with `set`, and return the map. The caller then deletes
/// the plaintext file or key. `set` must not write back to the plaintext location.
pub fn migrate_plain(
    plain: BTreeMap<String, String>,
    mut set: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<BTreeMap<String, String>, String> {
    for (id, password) in &plain {
        if valid_account_id(id) && !password.is_empty() {
            set(id, password)?;
        }
    }
    Ok(plain)
}

/// Pair each leftover plaintext password with the username already on the account, if any.
pub fn hosted_migration_entries(
    plain: &BTreeMap<String, String>,
    usernames: &BTreeMap<String, String>,
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (id, password) in plain {
        if valid_account_id(id) && !password.is_empty() {
            let username = usernames.get(id).cloned().unwrap_or_default();
            out.push((id.clone(), username, password.clone()));
        }
    }
    out
}

/// Load, migrate leftover plaintext, and return the passwords this device still keeps in memory
/// (none when the host stores them).
pub async fn startup(
    hosted: bool,
    account_ids: &[String],
    usernames: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    if hosted {
        startup_hosted(usernames).await
    } else {
        startup_local(account_ids).await
    }
}

async fn startup_hosted(
    usernames: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    // Move any leftover plaintext into the server vault, then drop it from this browser.
    if let Some(text) = host::load(KEY_SECRETS) {
        if let SecretsFile::Plain(plain) = parse_secrets_file(&text) {
            for (id, username, password) in hosted_migration_entries(&plain, usernames) {
                put_hosted(&id, &username, &password).await?;
            }
        }
        host::remove(KEY_SECRETS);
    }
    Ok(BTreeMap::new())
}

#[cfg(not(target_arch = "wasm32"))]
async fn startup_local(account_ids: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    if let Some(text) = host::load(KEY_SECRETS) {
        if let SecretsFile::Plain(plain) = parse_secrets_file(&text) {
            migrate_plain(plain.clone(), set_local_blocking)?;
            // Native: delete the plaintext file. Web: replace it with ciphertext in set_local,
            // then still remove any leftover plaintext by writing the encrypted form only
            // (already done) — if we are native, remove the file; if web, `set` already
            // rewrote the key, but a failed encrypt should not delete. After a successful
            // migrate we drop the plaintext key on native; on web `flush_encrypted` did it.
            #[cfg(not(target_arch = "wasm32"))]
            host::remove(KEY_SECRETS);
            out.extend(plain);
        }
    }
    for id in account_ids {
        if out.contains_key(id) {
            continue;
        }
        if let Some(password) = get_local(id).await? {
            out.insert(id.clone(), password);
        }
    }
    Ok(out)
}

/// Persist a password for a non-hosted account.
pub async fn set(account_id: &str, password: &str) -> Result<(), String> {
    if !valid_account_id(account_id) {
        return Err("that account id is not valid".into());
    }
    set_local(account_id, password).await
}

pub async fn remove(account_id: &str) -> Result<(), String> {
    remove_local(account_id).await
}

/// Store (or replace) a remote calendar password on the artcraftCloud server.
pub async fn put_hosted(account_id: &str, username: &str, password: &str) -> Result<(), String> {
    if !valid_account_id(account_id) {
        return Err("that account id is not valid".into());
    }
    hosted_request("PUT", account_id, Some(username), Some(password)).await
}

pub async fn delete_hosted(account_id: &str) -> Result<(), String> {
    if !valid_account_id(account_id) {
        return Err("that account id is not valid".into());
    }
    hosted_request("DELETE", account_id, None, None).await
}

async fn hosted_request(
    method: &str,
    account_id: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<(), String> {
    let url = format!("/api/calendar/remote/credentials/{account_id}");
    let body = password.map(|password| {
        serde_json::json!({ "username": username.unwrap_or(""), "password": password }).to_string()
    });
    let mut headers = vec![("x-requested-with".into(), "artcraft".into())];
    if body.is_some() {
        headers.push(("content-type".into(), "application/json".into()));
    }
    let resp = crate::net::send(crate::net::HttpRequest {
        method: method.into(),
        url,
        headers,
        body,
    })
    .await?;
    if (200..300).contains(&resp.status) || resp.status == 404 && method == "DELETE" {
        return Ok(());
    }
    Err(if resp.body.trim().is_empty() {
        format!("the host answered {}", resp.status)
    } else {
        resp.body
    })
}

// ---------------------------------------------------------------------------------------------
// Native: OS credential store

#[cfg(not(target_arch = "wasm32"))]
fn set_local_blocking(account_id: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, account_id).map_err(|e| e.to_string())?;
    entry.set_password(password).map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn set_local(account_id: &str, password: &str) -> Result<(), String> {
    set_local_blocking(account_id, password)
}

#[cfg(not(target_arch = "wasm32"))]
async fn get_local(account_id: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, account_id).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn remove_local(account_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, account_id).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

// ---------------------------------------------------------------------------------------------
// Standalone web: WebCrypto AES-GCM + IndexedDB key, ciphertext in localStorage

#[cfg(target_arch = "wasm32")]
async fn startup_local(account_ids: &[String]) -> Result<BTreeMap<String, String>, String> {
    startup_web(account_ids).await
}

#[cfg(target_arch = "wasm32")]
async fn startup_web(account_ids: &[String]) -> Result<BTreeMap<String, String>, String> {
    let key = master_key().await?;
    let mut file = match host::load(KEY_SECRETS)
        .map(|t| parse_secrets_file(&t))
        .unwrap_or(SecretsFile::Empty)
    {
        SecretsFile::Plain(plain) => {
            let mut file = EncryptedFile {
                v: 1,
                alg: "AES-GCM".into(),
                accounts: BTreeMap::new(),
            };
            for (id, password) in &plain {
                if valid_account_id(id) && !password.is_empty() {
                    file.accounts
                        .insert(id.clone(), encrypt_entry(&key, id, password).await?);
                }
            }
            save_encrypted(&file);
            file
        }
        SecretsFile::Encrypted(file) => file,
        SecretsFile::Empty => EncryptedFile {
            v: 1,
            alg: "AES-GCM".into(),
            accounts: BTreeMap::new(),
        },
    };
    let mut out = BTreeMap::new();
    let ids: Vec<String> = file
        .accounts
        .keys()
        .cloned()
        .chain(account_ids.iter().cloned())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(entry) = file.accounts.get(&id) {
            match decrypt_entry(&key, &id, entry).await {
                Ok(password) => {
                    out.insert(id, password);
                }
                Err(_) => {
                    file.accounts.remove(&id);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
async fn set_local(account_id: &str, password: &str) -> Result<(), String> {
    let key = master_key().await?;
    let mut file = load_encrypted();
    file.accounts.insert(
        account_id.to_string(),
        encrypt_entry(&key, account_id, password).await?,
    );
    save_encrypted(&file);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn remove_local(account_id: &str) -> Result<(), String> {
    let mut file = load_encrypted();
    file.accounts.remove(account_id);
    if file.accounts.is_empty() {
        host::remove(KEY_SECRETS);
    } else {
        save_encrypted(&file);
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn load_encrypted() -> EncryptedFile {
    match host::load(KEY_SECRETS)
        .map(|t| parse_secrets_file(&t))
        .unwrap_or(SecretsFile::Empty)
    {
        SecretsFile::Encrypted(file) => file,
        _ => EncryptedFile {
            v: 1,
            alg: "AES-GCM".into(),
            accounts: BTreeMap::new(),
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn save_encrypted(file: &EncryptedFile) {
    host::save(
        KEY_SECRETS,
        &serde_json::to_string(file)
            .unwrap_or_else(|_| "{\"v\":1,\"alg\":\"AES-GCM\",\"accounts\":{}}".into()),
    );
}

#[cfg(target_arch = "wasm32")]
async fn encrypt_entry(
    key: &web_sys::CryptoKey,
    account_id: &str,
    password: &str,
) -> Result<EncryptedEntry, String> {
    use base64::Engine;
    let iv = random_bytes(12)?;
    let ct = aes_gcm(key, &iv, account_id.as_bytes(), password.as_bytes(), true).await?;
    Ok(EncryptedEntry {
        n: base64::engine::general_purpose::STANDARD.encode(iv),
        c: base64::engine::general_purpose::STANDARD.encode(ct),
    })
}

#[cfg(target_arch = "wasm32")]
async fn decrypt_entry(
    key: &web_sys::CryptoKey,
    account_id: &str,
    entry: &EncryptedEntry,
) -> Result<String, String> {
    use base64::Engine;
    let iv = base64::engine::general_purpose::STANDARD
        .decode(entry.n.trim())
        .map_err(|_| "stored secret is damaged".to_string())?;
    let ct = base64::engine::general_purpose::STANDARD
        .decode(entry.c.trim())
        .map_err(|_| "stored secret is damaged".to_string())?;
    let plain = aes_gcm(key, &iv, account_id.as_bytes(), &ct, false).await?;
    String::from_utf8(plain).map_err(|_| "stored secret is damaged".into())
}

#[cfg(target_arch = "wasm32")]
fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let crypto = window.crypto().map_err(|_| "no WebCrypto")?;
    let mut buf = vec![0u8; n];
    crypto
        .get_random_values_with_u8_array(&mut buf)
        .map_err(|_| "no random bytes".to_string())?;
    Ok(buf)
}

#[cfg(target_arch = "wasm32")]
async fn aes_gcm(
    key: &web_sys::CryptoKey,
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
    encrypt: bool,
) -> Result<Vec<u8>, String> {
    use wasm_bindgen_futures::JsFuture;
    let window = web_sys::window().ok_or("no window")?;
    let subtle = window.crypto().map_err(|_| "no WebCrypto")?.subtle();
    let mut iv_buf = iv.to_vec();
    let mut aad_buf = aad.to_vec();
    let params = web_sys::AesGcmParams::new_with_u8_slice("AES-GCM", &mut iv_buf);
    params.set_additional_data_u8_slice(&mut aad_buf);
    let promise = if encrypt {
        subtle
            .encrypt_with_object_and_u8_array(params.as_ref(), key, data)
            .map_err(|e| format!("{e:?}"))?
    } else {
        subtle
            .decrypt_with_object_and_u8_array(params.as_ref(), key, data)
            .map_err(|e| format!("{e:?}"))?
    };
    let out = JsFuture::from(promise)
        .await
        .map_err(|_| "the stored secret could not be read".to_string())?;
    Ok(js_sys::Uint8Array::new(&out).to_vec())
}

#[cfg(target_arch = "wasm32")]
async fn master_key() -> Result<web_sys::CryptoKey, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let db = open_keys_db().await?;
    if let Some(key) = idb_get_key(&db).await? {
        return Ok(key);
    }
    let window = web_sys::window().ok_or("no window")?;
    let subtle = window.crypto().map_err(|_| "no WebCrypto")?.subtle();
    let params = web_sys::AesKeyGenParams::new("AES-GCM", 256);
    let usages = js_sys::Array::new();
    usages.push(&"encrypt".into());
    usages.push(&"decrypt".into());
    let promise = subtle
        .generate_key_with_object(params.as_ref(), false, &usages)
        .map_err(|e| format!("{e:?}"))?;
    let key: web_sys::CryptoKey = JsFuture::from(promise)
        .await
        .map_err(|e| format!("{e:?}"))?
        .dyn_into()
        .map_err(|_| "not a CryptoKey".to_string())?;
    idb_put_key(&db, &key).await?;
    Ok(key)
}

#[cfg(target_arch = "wasm32")]
async fn open_keys_db() -> Result<web_sys::IdbDatabase, String> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window().ok_or("no window")?;
    let factory = window
        .indexed_db()
        .map_err(|_| "no IndexedDB")?
        .ok_or("no IndexedDB")?;
    let open = factory
        .open_with_u32("calendarcraft-keys", 1)
        .map_err(|e| format!("{e:?}"))?;
    // Create the store on first open.
    {
        let on_upgrade =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
                let Some(target) = event.target() else { return };
                let Ok(req) = target.dyn_into::<web_sys::IdbOpenDbRequest>() else {
                    return;
                };
                let Ok(db) = req
                    .result()
                    .and_then(|v| v.dyn_into::<web_sys::IdbDatabase>())
                else {
                    return;
                };
                if !db.object_store_names().contains("keys") {
                    let _ = db.create_object_store("keys");
                }
            }) as Box<dyn FnMut(web_sys::Event)>);
        open.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
        on_upgrade.forget();
    }
    let value = idb_request(open.unchecked_into()).await?;
    value.dyn_into().map_err(|_| "not a database".to_string())
}

#[cfg(target_arch = "wasm32")]
async fn idb_get_key(db: &web_sys::IdbDatabase) -> Result<Option<web_sys::CryptoKey>, String> {
    use wasm_bindgen::JsCast;
    let tx = db
        .transaction_with_str("keys")
        .map_err(|e| format!("{e:?}"))?;
    let store = tx.object_store("keys").map_err(|e| format!("{e:?}"))?;
    let req = store
        .get(&wasm_bindgen::JsValue::from_str("master"))
        .map_err(|e| format!("{e:?}"))?;
    let value = idb_request(req).await?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(value.dyn_into().map_err(|_| {
        "stored key is not a CryptoKey".to_string()
    })?))
}

#[cfg(target_arch = "wasm32")]
async fn idb_put_key(db: &web_sys::IdbDatabase, key: &web_sys::CryptoKey) -> Result<(), String> {
    let tx = db
        .transaction_with_str_and_mode("keys", web_sys::IdbTransactionMode::Readwrite)
        .map_err(|e| format!("{e:?}"))?;
    let store = tx.object_store("keys").map_err(|e| format!("{e:?}"))?;
    let req = store
        .put_with_key(key, &wasm_bindgen::JsValue::from_str("master"))
        .map_err(|e| format!("{e:?}"))?;
    let _ = idb_request(req).await?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn idb_request(req: web_sys::IdbRequest) -> Result<wasm_bindgen::JsValue, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let reject2 = reject.clone();
        let on_ok = wasm_bindgen::closure::Closure::once(Box::new(move |event: web_sys::Event| {
            let result = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::IdbRequest>().ok())
                .and_then(|r| r.result().ok())
                .unwrap_or(wasm_bindgen::JsValue::UNDEFINED);
            let _ = resolve.call1(&wasm_bindgen::JsValue::UNDEFINED, &result);
        }));
        let on_err =
            wasm_bindgen::closure::Closure::once(Box::new(move |_event: web_sys::Event| {
                let _ = reject2.call1(
                    &wasm_bindgen::JsValue::UNDEFINED,
                    &wasm_bindgen::JsValue::from_str("IndexedDB failed"),
                );
            }));
        req.set_onsuccess(Some(on_ok.as_ref().unchecked_ref()));
        req.set_onerror(Some(on_err.as_ref().unchecked_ref()));
        on_ok.forget();
        on_err.forget();
    });
    JsFuture::from(promise)
        .await
        .map_err(|_| "IndexedDB failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids_are_plain() {
        assert!(valid_account_id("acc_a1b2"));
        assert!(valid_account_id("artcraft"));
        assert!(!valid_account_id(""));
        assert!(!valid_account_id("../etc"));
        assert!(!valid_account_id("acc/x"));
        assert!(!valid_account_id(&"a".repeat(65)));
        assert!(!valid_account_id("_hidden"));
    }

    #[test]
    fn plaintext_maps_are_detected() {
        let plain = r#"{"acc_one":"s3cret","acc_two":"other"}"#;
        assert!(is_plaintext(plain));
        match parse_secrets_file(plain) {
            SecretsFile::Plain(map) => {
                assert_eq!(map.get("acc_one").map(String::as_str), Some("s3cret"));
                assert_eq!(map.len(), 2);
            }
            other => panic!("{other:?}"),
        }
        let enc = r#"{"v":1,"alg":"AES-GCM","accounts":{"acc_one":{"n":"AAAA","c":"BBBB"}}}"#;
        assert!(!is_plaintext(enc));
        assert!(matches!(parse_secrets_file(enc), SecretsFile::Encrypted(_)));
        assert!(matches!(parse_secrets_file(""), SecretsFile::Empty));
        assert!(matches!(parse_secrets_file("{}"), SecretsFile::Empty));
    }

    #[test]
    fn migrate_moves_each_entry() {
        let plain = BTreeMap::from([
            ("acc_one".into(), "s3cret".into()),
            ("acc_two".into(), "other".into()),
            ("bad/id".into(), "no".into()),
        ]);
        let mut stored = BTreeMap::new();
        let out = migrate_plain(plain.clone(), |id, password| {
            stored.insert(id.to_string(), password.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(out.get("acc_one").map(String::as_str), Some("s3cret"));
        assert_eq!(stored.get("acc_one").map(String::as_str), Some("s3cret"));
        assert_eq!(stored.get("acc_two").map(String::as_str), Some("other"));
        assert!(!stored.contains_key("bad/id"), "invalid ids are not copied");
        assert!(!stored.values().any(|v| v.is_empty()));
    }

    #[test]
    fn hosted_migration_takes_the_username_from_the_account() {
        let plain = BTreeMap::from([
            ("acc_one".into(), "s3cret".into()),
            ("acc_two".into(), "other".into()),
            ("bad/id".into(), "no".into()),
        ]);
        let usernames = BTreeMap::from([("acc_one".into(), "ada@example.test".into())]);
        let entries = hosted_migration_entries(&plain, &usernames);
        assert_eq!(
            entries,
            vec![
                ("acc_one".into(), "ada@example.test".into(), "s3cret".into()),
                ("acc_two".into(), String::new(), "other".into()),
            ]
        );
    }

    #[test]
    fn encrypted_file_does_not_carry_plaintext_passwords() {
        let file = EncryptedFile {
            v: 1,
            alg: "AES-GCM".into(),
            accounts: BTreeMap::from([(
                "acc_one".into(),
                EncryptedEntry {
                    n: "bm9uY2U=".into(),
                    c: "Y2lwaGVy".into(),
                },
            )]),
        };
        let text = serde_json::to_string(&file).unwrap();
        assert!(!text.contains("s3cret"));
        assert!(text.contains("\"v\":1"));
        assert!(is_plaintext(r#"{"acc_one":"s3cret"}"#));
        assert!(!is_plaintext(&text));
    }

    #[test]
    fn native_plaintext_file_is_deleted_after_a_dry_migrate() {
        // The file format and the "move then delete" order, without talking to the OS store.
        let path = std::env::temp_dir().join(format!(
            "calendarcraft.secrets.test.{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"acc_one":"s3cret"}"#).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(is_plaintext(&text));
        let mut stored = BTreeMap::new();
        if let SecretsFile::Plain(plain) = parse_secrets_file(&text) {
            migrate_plain(plain, |id, password| {
                stored.insert(id.to_string(), password.to_string());
                Ok(())
            })
            .unwrap();
            std::fs::remove_file(&path).unwrap();
        }
        assert_eq!(stored.get("acc_one").map(String::as_str), Some("s3cret"));
        assert!(
            !path.exists(),
            "plaintext file is gone after a successful move"
        );
    }
}
