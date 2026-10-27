//! Joining a vault with a connection code: the first device creates the vault, later devices prove they hold its master key.

use crate::conn::ConnectionCode;
use crate::error::ClientError;
use crate::session::db_path;
use crate::sqlite::SqliteStore;
use crate::transport::Transport;
use base64::{engine::general_purpose::STANDARD as B64, engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use onepaste_core::crypto::{self, aad, KdfParams};
use onepaste_core::keys::{vault_proof, KeyParams};
use onepaste_core::{passphrase, recovery};
use onepaste_sync::{HttpRequest, Method};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct JoinOptions {
    pub code: String,
    pub device_name: String,
    /// Required to join an existing vault. For a new vault it is optional: a random one is generated when absent.
    pub passphrase: Option<String>,
    pub recovery_kit_out: Option<PathBuf>,
}

pub struct JoinResult {
    pub device_id: String,
    pub new_vault: bool,
    /// Only for a new vault where no passphrase was given. It is shown once and never stored.
    pub generated_passphrase: Option<String>,
    /// Only for a new vault with no output file.
    pub recovery_kit: Option<String>,
}

fn request(method: Method, path: &str, extra: Vec<(&str, String)>, body: Option<Value>) -> HttpRequest {
    let mut headers = vec![("OnePaste-Proto".to_string(), "1".to_string())];
    headers.extend(extra.into_iter().map(|(k, v)| (k.to_string(), v)));
    HttpRequest { method, path_and_query: path.to_string(), headers, body: body.map(|b| b.to_string().into_bytes()).unwrap_or_default() }
}

fn code_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body).ok().and_then(|v| v.get("code")?.as_str().map(str::to_string)).unwrap_or_else(|| "unknown".into())
}

/// Writes a new file atomically: a temporary file in the same directory (mode 0600), `fsync`, then a link to the final
/// name, which fails if the name is taken, so an existing file is never overwritten. Finally the directory is synced.
fn write_new_private(path: &Path, data: &str) -> Result<(), ClientError> {
    use std::io::Write;
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| ClientError::Other("invalid recovery kit path".into()))?;
    let tmp = dir.join(format!(".{}.tmp", name.to_string_lossy()));
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(data.as_bytes())?;
        f.sync_all()?;
    }
    crate::crash::crash_point("during_kit_write");
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(ClientError::KitFileExists),
        Err(e) => return Err(e.into()),
    }
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

pub async fn join(dir: &Path, opts: JoinOptions) -> Result<JoinResult, ClientError> {
    let code = ConnectionCode::parse(&opts.code)?;
    // Refuse before anything else, and before any network request, to overwrite an existing file
    if opts.recovery_kit_out.as_ref().is_some_and(|p| p.symlink_metadata().is_ok()) {
        return Err(ClientError::KitFileExists);
    }
    let mut store = SqliteStore::open(&db_path(dir))?;
    if store.meta_get("device_id")?.is_some() {
        return Err(ClientError::AlreadyJoined);
    }
    let transport = Transport::new(&code.u, &code.fp);

    // 1. Ask for the key parameter record with the registration credential alone
    let got = transport.send(&request(Method::Get, "/v1/vault/keyparams", vec![("OnePaste-Reg-Token", code.t.clone())], None)).await?;
    let device_id = onepaste_core::id::new_id()?;
    let (record, mk, new_vault, generated, kit) = match got.status {
        200 => {
            let pass = opts.passphrase.as_deref().ok_or(ClientError::PassphraseRequired)?;
            let record: KeyParams = serde_json::from_slice(&got.body).map_err(|_| ClientError::Other("malformed key parameter record".into()))?;
            // A wrong passphrase stops here, before anything is registered
            let mk = record.unlock(pass)?;
            record.verify_mk(&mk)?;
            (record, mk, false, None, None)
        }
        404 if code_of(&got.body) == "vault_empty" => {
            let (pass, generated) = match opts.passphrase {
                Some(p) => (p, None),
                None => {
                    let p = passphrase::generate()?;
                    (p.clone(), Some(p))
                }
            };
            let (record, mk) = KeyParams::create(&pass, KdfParams::DEFAULT)?;
            let kit = recovery::encode(&mk);
            (record, mk, true, generated, Some(kit))
        }
        status => return Err(ClientError::Server { status, code: code_of(&got.body) }),
    };

    // 2. Keep the recovery kit safe before the vault exists on the server
    let mut kit_for_output = None;
    if let Some(kit) = &kit {
        match &opts.recovery_kit_out {
            Some(path) => write_new_private(path, &format!("{kit}\n"))?,
            None => kit_for_output = Some(kit.clone()),
        }
    }

    // 3. Register the device
    let sealed_name = B64.encode(crypto::seal(&mk, &aad::device(&device_id), opts.device_name.as_bytes())?);
    let proof = B64URL.encode(vault_proof(&mk));
    let mut body = json!({ "reg_token": code.t, "device_id": device_id, "sealed_name": sealed_name });
    if new_vault {
        body["init"] = json!({ "record": record, "vault_proof": proof });
    } else {
        body["vault_proof"] = json!(proof);
    }
    let reg = transport
        .send(&request(Method::Post, "/v1/devices/register", vec![("Idempotency-Key", onepaste_core::id::new_id()?), ("Content-Type", "application/json".into())], Some(body)))
        .await?;
    if reg.status != 200 {
        return Err(ClientError::Server { status: reg.status, code: code_of(&reg.body) });
    }
    let token = serde_json::from_slice::<Value>(&reg.body)
        .ok()
        .and_then(|v| v.get("device_token")?.as_str().map(str::to_string))
        .ok_or_else(|| ClientError::Other("malformed registration answer".into()))?;

    // 4. Save everything the device needs in one transaction; the token is kept encrypted
    let sealed_token = crypto::seal(&mk, aad::DEVTOKEN, token.as_bytes())?;
    let record_json = serde_json::to_string(&record).map_err(|_| ClientError::Other("record".into()))?;
    store.save(
        &[("u", code.u.as_str()), ("fp", code.fp.as_str()), ("device_id", device_id.as_str()), ("keyparams", record_json.as_str())],
        &[("devtoken", sealed_token.as_slice())],
    )?;
    Ok(JoinResult { device_id, new_vault, generated_passphrase: generated, recovery_kit: kit_for_output })
}
