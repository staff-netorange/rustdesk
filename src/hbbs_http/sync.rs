use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(not(any(target_os = "ios")))]
use crate::{common::API_LOG_INTERVAL, ui_interface::get_builtin_option, Connection};
use hbb_common::{
    config::{self, Config, LocalConfig},
    log,
    tokio::{self, sync::broadcast, time::Instant},
};
use base::config::keys;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const TIME_HEARTBEAT: Duration = Duration::from_secs(15);
const UPLOAD_SYSINFO_TIMEOUT: Duration = Duration::from_secs(120);
const TIME_CONN: Duration = Duration::from_secs(3);

#[cfg(not(any(target_os = "ios")))]
lazy_static::lazy_static! {
    static ref SENDER : Mutex<broadcast::Sender<Vec<i32>>> = Mutex::new(start_hbbs_sync());
    static ref PRO: Arc<Mutex<bool>> = Default::default();
}

#[cfg(not(any(target_os = "ios")))]
pub fn start() {
    let _sender = SENDER.lock().unwrap();
}

#[cfg(not(target_os = "ios"))]
pub fn signal_receiver() -> broadcast::Receiver<Vec<i32>> {
    SENDER.lock().unwrap().subscribe()
}

#[cfg(not(any(target_os = "ios")))]
fn start_hbbs_sync() -> broadcast::Sender<Vec<i32>> {
    let (tx, _rx) = broadcast::channel::<Vec<i32>>(16);
    std::thread::spawn(move || start_hbbs_sync_async());
    return tx;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StrategyOptions {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config_options: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, String>,
}

struct InfoUploaded {
    uploaded: bool,
    url: String,
    last_uploaded: Option<Instant>,
    id: String,
    username: Option<String>,
}

impl Default for InfoUploaded {
    fn default() -> Self {
        Self {
            uploaded: false,
            url: "".to_owned(),
            last_uploaded: None,
            id: "".to_owned(),
            username: None,
        }
    }
}

impl InfoUploaded {
    fn uploaded(url: String, id: String, username: String) -> Self {
        Self {
            uploaded: true,
            url,
            last_uploaded: None,
            id,
            username: Some(username),
        }
    }
}

#[cfg(not(any(target_os = "ios")))]
#[tokio::main(flavor = "current_thread")]
async fn start_hbbs_sync_async() {
    let mut interval = crate::rustdesk_interval(tokio::time::interval_at(
        Instant::now() + TIME_CONN,
        TIME_CONN,
    ));
    let mut last_sent: Option<Instant> = None;
    let mut info_uploaded = InfoUploaded::default();
    let mut sysinfo_ver = "".to_owned();
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // NETAssist: applica la scadenza locale del servizio temporaneo anche se il portale non risponde
                netassist::tick();
                let url = heartbeat_url();
                let id = Config::get_id();
                if url.is_empty() {
                    *PRO.lock().unwrap() = false;
                    continue;
                }
                if config::option2bool("stop-service", &Config::get_option("stop-service")) {
                    continue;
                }
                let conns = Connection::alive_conns();
                if info_uploaded.uploaded && (url != info_uploaded.url || id != info_uploaded.id) {
                    info_uploaded.uploaded = false;
                    *PRO.lock().unwrap() = false;
                }
                // For Windows:
                // We can't skip uploading sysinfo when the username is empty, because the username may
                // always be empty before login. We also need to upload the other sysinfo info.
                //
                // https://github.com/rustdesk/rustdesk/discussions/8031
                // We still need to check the username after uploading sysinfo, because
                // 1. The username may be empty when logining in, and it can be fetched after a while.
                //    In this case, we need to upload sysinfo again.
                // 2. The username may be changed after uploading sysinfo, and we need to upload sysinfo again.
                //
                // The Windows session will switch to the last user session before the restart,
                // so it may be able to get the username before login.
                // But strangely, sometimes we can get the username before login,
                // we may not be able to get the username before login after the next restart.
                let mut v = crate::get_sysinfo();
                let sys_username = v["username"].as_str().unwrap_or_default().to_string();
                // Though the username comparison is only necessary on Windows,
                // we still keep the comparison on other platforms for consistency.
                let need_upload = (!info_uploaded.uploaded || info_uploaded.username.as_ref() != Some(&sys_username)) &&
                    info_uploaded.last_uploaded.map(|x| x.elapsed() >= UPLOAD_SYSINFO_TIMEOUT).unwrap_or(true);
                if need_upload {
                    v["version"] = json!(crate::VERSION);
                    v["id"] = json!(id);
                    v["uuid"] = json!(crate::encode64(hbb_common::get_uuid()));
                    let ab_name = Config::get_option(keys::OPTION_PRESET_ADDRESS_BOOK_NAME);
                    if !ab_name.is_empty() {
                        v[keys::OPTION_PRESET_ADDRESS_BOOK_NAME] = json!(ab_name);
                    }
                    let ab_tag = Config::get_option(keys::OPTION_PRESET_ADDRESS_BOOK_TAG);
                    if !ab_tag.is_empty() {
                        v[keys::OPTION_PRESET_ADDRESS_BOOK_TAG] = json!(ab_tag);
                    }
                    let ab_alias = Config::get_option(keys::OPTION_PRESET_ADDRESS_BOOK_ALIAS);
                    if !ab_alias.is_empty() {
                        v[keys::OPTION_PRESET_ADDRESS_BOOK_ALIAS] = json!(ab_alias);
                    }
                    let ab_password = Config::get_option(keys::OPTION_PRESET_ADDRESS_BOOK_PASSWORD);
                    if !ab_password.is_empty() {
                        v[keys::OPTION_PRESET_ADDRESS_BOOK_PASSWORD] = json!(ab_password);
                    }
                    let ab_note = Config::get_option(keys::OPTION_PRESET_ADDRESS_BOOK_NOTE);
                    if !ab_note.is_empty() {
                        v[keys::OPTION_PRESET_ADDRESS_BOOK_NOTE] = json!(ab_note);
                    }
                    let username = get_builtin_option(keys::OPTION_PRESET_USERNAME);
                    if !username.is_empty() {
                        v[keys::OPTION_PRESET_USERNAME] = json!(username);
                    }
                    let strategy_name = get_builtin_option(keys::OPTION_PRESET_STRATEGY_NAME);
                    if !strategy_name.is_empty() {
                        v[keys::OPTION_PRESET_STRATEGY_NAME] = json!(strategy_name);
                    }
                    let device_group_name = get_builtin_option(keys::OPTION_PRESET_DEVICE_GROUP_NAME);
                    if !device_group_name.is_empty() {
                        v[keys::OPTION_PRESET_DEVICE_GROUP_NAME] = json!(device_group_name);
                    }
                    let device_username = Config::get_option(keys::OPTION_PRESET_DEVICE_USERNAME);
                    if !device_username.is_empty() {
                        v["username"] = json!(device_username);
                    }
                    let device_name = Config::get_option(keys::OPTION_PRESET_DEVICE_NAME);
                    if !device_name.is_empty() {
                        v["hostname"] = json!(device_name);
                    }
                    let note = Config::get_option(keys::OPTION_PRESET_NOTE);
                    if !note.is_empty() {
                        v[keys::OPTION_PRESET_NOTE] = json!(note);
                    }
                    let v = v.to_string();
                    let mut hash = "".to_owned();
                    if crate::is_public(&url) {
                        use sha2::{Digest, Sha256};
                        let mut hasher = Sha256::new();
                        hasher.update(url.as_bytes());
                        hasher.update(&v.as_bytes());
                        let res = hasher.finalize();
                        hash = hbb_common::base64::encode(&res[..]);
                        let old_hash = config::Status::get("sysinfo_hash");
                        let ver = config::Status::get("sysinfo_ver"); // sysinfo_ver is the version of sysinfo on server's side
                        if hash == old_hash {
                            // When the api doesn't exist, Ok("") will be returned in test.
                            let samever = match crate::post_request(url.replace("heartbeat", "sysinfo_ver"), "".to_owned(), "").await {
                                Ok(x)  => {
                                    sysinfo_ver = x.clone();
                                    *PRO.lock().unwrap() = true;
                                    x == ver
                                }
                                _ => {
                                    false // to make sure Pro can be assigned in below post for old
                                            // hbbs pro not supporting sysinfo_ver, use false for ensuring
                                }
                            };
                            if samever {
                                info_uploaded = InfoUploaded::uploaded(url.clone(), id.clone(), sys_username);
                                log::info!("sysinfo not changed, skip upload");
                                continue;
                            }
                        }
                    }
                    match crate::post_request(url.replace("heartbeat", "sysinfo"), v, "").await {
                        Ok(x)  => {
                            if x == "SYSINFO_UPDATED" {
                                info_uploaded = InfoUploaded::uploaded(url.clone(), id.clone(), sys_username);
                                log::info!("sysinfo updated");
                                if !hash.is_empty() {
                                    config::Status::set("sysinfo_hash", hash);
                                    config::Status::set("sysinfo_ver", sysinfo_ver.clone());
                                }
                                *PRO.lock().unwrap() = true;
                            } else if x == "ID_NOT_FOUND" {
                                info_uploaded.last_uploaded = None; // next heartbeat will upload sysinfo again
                            } else {
                                info_uploaded.last_uploaded = Some(Instant::now());
                            }
                        }
                        _ => {
                            info_uploaded.last_uploaded = Some(Instant::now());
                        }
                    }
                }
                if conns.is_empty() && last_sent.map(|x| x.elapsed() < TIME_HEARTBEAT).unwrap_or(false) {
                    continue;
                }
                last_sent = Some(Instant::now());
                let mut v = Value::default();
                v["id"] = json!(id);
                v["uuid"] = json!(crate::encode64(hbb_common::get_uuid()));
                v["ver"] = json!(hbb_common::get_version_number(crate::VERSION));
                if !conns.is_empty() {
                    v["conns"] = json!(conns);
                }
                let modified_at = LocalConfig::get_option("strategy_timestamp").parse::<i64>().unwrap_or(0);
                v["modified_at"] = json!(modified_at);
                let response = crate::post_request(url.clone(), v.to_string(), "").await;
                if let Err(err) = &response {
                    hbb_common::throttled_log!(API_LOG_INTERVAL, warn, "Heartbeat failed: {err:?}");
                }
                if let Ok(s) = response {
                    if let Ok(mut rsp) = serde_json::from_str::<HashMap::<&str, Value>>(&s) {
                        if rsp.remove("sysinfo").is_some() {
                            info_uploaded.uploaded = false;
                            config::Status::set("sysinfo_hash", "".to_owned());
                            log::info!("sysinfo required to forcely update");
                        }
                        if let Some(conns)  = rsp.remove("disconnect") {
                                if let Ok(conns) = serde_json::from_value::<Vec<i32>>(conns) {
                                    SENDER.lock().unwrap().send(conns).ok();
                                }
                        }
                        if let Some(strategy) = rsp.remove("strategy") {
                            if let Ok(strategy) = serde_json::from_value::<StrategyOptions>(strategy) {
                                if let Err(error) = handle_config_options(strategy.config_options).await {
                                    hbb_common::throttled_log!(API_LOG_INTERVAL, error, "Strategy update rejected: {error}");
                                    continue;
                                }
                                log::info!("strategy updated");
                            }
                        }
                        // NETAssist: comando del portale: servizio temporaneo e stato sessione
                        if let Some(na) = rsp.remove("netassist") {
                            netassist::apply(&na);
                        }
                        if let Some(rsp_modified_at) = rsp.remove("modified_at") {
                            if let Ok(rsp_modified_at) = serde_json::from_value::<i64>(rsp_modified_at) {
                                if rsp_modified_at != modified_at {
                                    LocalConfig::set_option("strategy_timestamp".to_string(), rsp_modified_at.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// NETAssist: servizio temporaneo automatico e stato della sessione.
// Il portale, nella risposta all'heartbeat, puo' inviare un oggetto "netassist":
//   { "active": bool, "service": "temporary"|"none", "until": <epoch ms> }
// "active" governa la password di sessione (vedi server/connection.rs);
// "service"/"until" governano il servizio temporaneo su Windows.
//
// Comportamento (Windows), durante una sessione attiva:
//  - cliente amministratore (o client gia' elevato): install silenzioso come servizio
//    Windows -> Ctrl+Alt+Canc, disconnessione utente, schermata di accesso e
//    riconnessione dopo il riavvio; auto-disinstallazione a fine sessione/scadenza.
//  - cliente non amministratore: servizio portable (SAS/UAC/schermata di accesso per la
//    sessione dal vivo) + autostart a livello utente (chiave Run di HKCU, nessun diritto
//    admin). L'autostart fa ripartire il client dopo il riavvio quando l'utente rientra,
//    cosi' la sessione non si perde (il PC torna online con lo stesso ID). Non da' il
//    controllo della schermata di accesso se nessuno effettua l'accesso: per quello serve
//    l'elevazione con credenziali admin dal tecnico. Autostart rimosso a fine sessione.
mod netassist {
    use hbb_common::config::Config;
    use serde_json::Value;

    pub fn apply(cmd: &Value) {
        let active = cmd.get("active").and_then(Value::as_bool).unwrap_or(false);
        let want = if active { "Y" } else { "" };
        if Config::get_option("netassist-session-active") != want {
            Config::set_option("netassist-session-active".to_owned(), want.to_owned());
        }
        #[cfg(windows)]
        service::drive(cmd);
    }

    #[cfg(windows)]
    pub fn tick() {
        service::enforce_deadline();
    }
    #[cfg(not(windows))]
    pub fn tick() {}

    #[cfg(windows)]
    mod service {
        use hbb_common::{config::Config, get_time, log};
        use serde_json::Value;
        use std::sync::atomic::{AtomicBool, Ordering};

        static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);
        static PORTABLE_TRIED: AtomicBool = AtomicBool::new(false);

        const FLAG_TEMP: &str = "netassist-temporary-service"; // "Y" se abbiamo installato un servizio temporaneo
        const FLAG_AUTOSTART: &str = "netassist-autostart";    // "Y" se abbiamo messo l'autostart utente
        const KEY_UNTIL: &str = "netassist-session-until";     // scadenza sessione in epoch ms

        fn set_opt(k: &str, v: &str) {
            if Config::get_option(k) != v {
                Config::set_option(k.to_owned(), v.to_owned());
            }
        }

        pub fn drive(cmd: &Value) {
            if let Some(until) = cmd.get("until").and_then(Value::as_i64) {
                if until > 0 {
                    set_opt(KEY_UNTIL, &until.to_string());
                }
            }
            match cmd.get("service").and_then(Value::as_str).unwrap_or("none") {
                "temporary" => ensure(),
                _ => teardown(),
            }
            enforce_deadline();
        }

        fn ensure() {
            if crate::platform::is_installed() && crate::platform::is_self_service_running() {
                set_opt(FLAG_TEMP, "Y");
                return;
            }
            let elevated = crate::platform::is_elevated(None).unwrap_or(false);
            if elevated {
                if INSTALL_TRIED.swap(true, Ordering::SeqCst) {
                    return;
                }
                set_opt(FLAG_TEMP, "Y");
                let opts = crate::platform::get_silent_install_options(Some(false));
                std::thread::spawn(move || {
                    log::info!("NETAssist: installo il servizio temporaneo per la sessione");
                    if let Err(e) = crate::platform::install_me(opts, "".to_owned(), true, false) {
                        log::error!("NETAssist: install del servizio temporaneo fallito: {}", e);
                        INSTALL_TRIED.store(false, Ordering::SeqCst);
                    }
                });
            } else {
                if PORTABLE_TRIED.swap(true, Ordering::SeqCst) {
                    return;
                }
                // Riconnessione dopo riavvio senza diritti admin: autostart a livello utente.
                set_user_autostart(true);
                set_opt(FLAG_AUTOSTART, "Y");
                use crate::portable_service::client;
                if !client::running() {
                    if let Err(e) = client::start_portable_service(client::StartPara::Direct) {
                        log::error!("NETAssist: avvio del servizio portable fallito: {}", e);
                        PORTABLE_TRIED.store(false, Ordering::SeqCst);
                    }
                }
            }
        }

        fn teardown() {
            cleanup();
        }

        pub fn enforce_deadline() {
            let temp = Config::get_option(FLAG_TEMP) == "Y";
            let autostart = Config::get_option(FLAG_AUTOSTART) == "Y";
            if !temp && !autostart {
                return;
            }
            let until = Config::get_option(KEY_UNTIL).parse::<i64>().unwrap_or(0);
            if until > 0 && get_time() >= until {
                log::info!("NETAssist: sessione scaduta, ripristino lo stato del client");
                cleanup();
            }
        }

        // Rimuove tutto cio' che e' stato predisposto per la sessione: autostart utente e,
        // se presente, il servizio temporaneo installato. Idempotente.
        fn cleanup() {
            INSTALL_TRIED.store(false, Ordering::SeqCst);
            PORTABLE_TRIED.store(false, Ordering::SeqCst);
            if Config::get_option(FLAG_AUTOSTART) == "Y" {
                set_user_autostart(false);
                set_opt(FLAG_AUTOSTART, "");
            }
            let was_temp = Config::get_option(FLAG_TEMP) == "Y";
            set_opt(FLAG_TEMP, "");
            set_opt(KEY_UNTIL, "");
            if was_temp && crate::platform::is_installed() {
                std::thread::spawn(|| {
                    log::info!("NETAssist: disinstallo il servizio temporaneo");
                    if let Err(e) = crate::platform::uninstall_me(false) {
                        log::error!("NETAssist: disinstallazione del servizio temporaneo fallita: {}", e);
                    }
                });
            }
        }

        // Autostart a livello utente (HKCU Run): non richiede diritti amministrativi.
        fn set_user_autostart(on: bool) {
            use winreg::{enums::HKEY_CURRENT_USER, RegKey};
            let name = format!("{} Session", crate::get_app_name());
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let run = match hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
                Ok((k, _)) => k,
                Err(e) => {
                    log::error!("NETAssist: apertura della chiave Run utente fallita: {}", e);
                    return;
                }
            };
            if on {
                let exe = std::env::current_exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if exe.is_empty() {
                    return;
                }
                if let Err(e) = run.set_value(&name, &format!("\"{}\"", exe)) {
                    log::error!("NETAssist: impostazione dell'autostart utente fallita: {}", e);
                }
            } else {
                let _ = run.delete_value(&name);
            }
        }
    }
}

fn heartbeat_url() -> String {
    let url = crate::common::get_api_server(
        Config::get_option("api-server"),
        Config::get_option("custom-rendezvous-server"),
    );
    if url.is_empty() || crate::is_public(&url) {
        return "".to_owned();
    }
    format!("{}/api/heartbeat", url)
}

async fn handle_config_options(
    config_options: HashMap<String, String>,
) -> hbb_common::ResultType<()> {
    let mut options = Config::get_options();
    let default_settings = config::DEFAULT_SETTINGS.read().unwrap().clone();
    config_options
        .iter()
        .map(|(k, v)| {
            // Priority: user config > default advanced options.
            // Only when default advanced options are also empty, remove user option (fallback to built-in default);
            // otherwise insert an empty value so user config remains present.
            if v.is_empty() && default_settings.get(k).map_or("", |v| v).is_empty() {
                options.remove(k);
            } else {
                options.insert(k.to_string(), v.to_string());
            }
        })
        .count();
    if crate::trusted_devices::config::changes_authentication(&options) {
        let current = Config::get_options();
        let mut non_auth_options = options.clone();
        for key in [
            "2fa",
            keys::OPTION_ENABLE_TRUSTED_DEVICES,
            keys::OPTION_TRUSTED_DEVICE_EXPIRATION,
        ] {
            match current.get(key) {
                Some(value) => {
                    non_auth_options.insert(key.to_owned(), value.clone());
                }
                None => {
                    non_auth_options.remove(key);
                }
            }
        }
        // Authentication update failures must not block unrelated strategy options.
        Config::set_options(non_auth_options);
        return crate::trusted_devices::config::update_options(options).await;
    }
    Config::set_options(options);
    Ok(())
}

#[allow(unused)]
#[cfg(not(any(target_os = "ios")))]
pub fn is_pro() -> bool {
    PRO.lock().unwrap().clone()
}

// Fire-and-forget by design: the switch flow must not block on this POST.
// If the device clock is outside the server's accepted window, the server
// returns its current Unix time and this task re-signs and retries once.
#[cfg(feature = "flutter")]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn register_switch_grant(switch_uuid: String) {
    tokio::spawn(async move {
        let api_server = crate::ui_interface::get_api_server();
        if api_server.is_empty() || crate::is_public(&api_server) {
            return;
        }
        use hbb_common::sodiumoxide::crypto::{hash::sha256, sign};
        let switch_code = crate::encode64(sha256::hash(switch_uuid.as_bytes()).0);
        let switch_code_verifier = switch_code_verifier(&switch_code);
        let timestamp = (hbb_common::get_time() / 1000).to_string();
        let id = Config::get_id();
        let kp = Config::get_key_pair();
        let Some(sk) = sign::SecretKey::from_slice(&kp.0) else {
            log::error!("Failed to register switch grant: no device key");
            return;
        };
        let url = format!("{}/api/switch-grant", api_server);
        let mut timestamp = timestamp;
        for attempt in 0..2 {
            let signature = sign::sign_detached(
                &switch_grant_signed_msg(&id, &switch_code_verifier, &timestamp),
                &sk,
            );
            let body = json!({
                "id": &id,
                "switch_code_verifier": &switch_code_verifier,
                "timestamp": &timestamp,
                "signature": crate::encode64(signature.to_bytes()),
            })
            .to_string();
            let response = match crate::post_request(url.clone(), body, "").await {
                Ok(response) => response,
                Err(e) => {
                    log::error!("Failed to register switch grant: {}", e);
                    return;
                }
            };
            let response = match serde_json::from_str::<Value>(&response) {
                Ok(response) => response,
                Err(e) => {
                    log::error!("Failed to register switch grant: invalid response: {}", e);
                    return;
                }
            };
            match response.get("accepted").and_then(Value::as_bool) {
                Some(true) => return,
                Some(false) => {}
                None => {
                    log::error!("Failed to register switch grant: missing accepted response");
                    return;
                }
            }
            let Some(server_time) = response["server_time"].as_i64() else {
                log::error!("Failed to register switch grant: rejected by server");
                return;
            };
            if attempt == 0 {
                log::warn!("Switch grant timestamp rejected, retrying with server time");
                timestamp = server_time.to_string();
            } else {
                log::error!("Failed to register switch grant after retrying with server time");
            }
        }
    });
}

#[cfg(feature = "flutter")]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn switch_code_verifier(switch_code: &str) -> String {
    use hbb_common::sodiumoxide::crypto::hash::sha256;

    let prefix = b"switch-grant-verifier\0";
    let mut msg = Vec::with_capacity(prefix.len() + switch_code.len());
    msg.extend_from_slice(prefix);
    msg.extend_from_slice(switch_code.as_bytes());
    crate::encode64(sha256::hash(&msg).0)
}

#[cfg(feature = "flutter")]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn switch_grant_signed_msg(id: &str, switch_code_verifier: &str, timestamp: &str) -> Vec<u8> {
    let mut msg =
        Vec::with_capacity(13 + id.len() + 1 + switch_code_verifier.len() + 1 + timestamp.len());
    msg.extend_from_slice(b"switch-grant\0");
    msg.extend_from_slice(id.as_bytes());
    msg.push(0);
    msg.extend_from_slice(switch_code_verifier.as_bytes());
    msg.push(0);
    msg.extend_from_slice(timestamp.as_bytes());
    msg
}

#[cfg(all(
    test,
    feature = "flutter",
    not(any(target_os = "android", target_os = "ios"))
))]
mod tests {
    use super::{switch_code_verifier, switch_grant_signed_msg};

    #[test]
    fn test_switch_code_verifier_is_not_raw_switch_code() {
        let switch_code = "code-abc";
        let verifier = switch_code_verifier(switch_code);
        assert_ne!(verifier, switch_code);
        assert_eq!(verifier, switch_code_verifier(switch_code));
        assert_eq!(
            verifier,
            "dMIn3uiPe77XodFB5IKi7PrKJ7l7+zVquNn0ObSaHQc="
        );
    }

    #[test]
    fn test_switch_grant_signed_msg_layout() {
        let expected: Vec<u8> = [
            &b"switch-grant\0"[..],
            b"id1",
            b"\0",
            b"c1",
            b"\0",
            b"1700000000",
        ]
        .concat();
        assert_eq!(switch_grant_signed_msg("id1", "c1", "1700000000"), expected);
    }
}
