//! `onepaste`: the OnePaste command line client.
//!
//! Exit codes: 0 success, 1 other error, 2 invalid arguments, 3 wrong passphrase or recovery kit, 4 the server was
//! rolled back, 5 this device was revoked, 6 queued changes are stuck (conflict or failed) after a sync.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::{ArgGroup, Parser, Subcommand};
use onepaste_client::{join, now_ms, ClientError, JoinOptions, RunEnd, Session};
use onepaste_core::crypto::{self, aad};
use onepaste_core::envelope::ItemPayload;
use onepaste_sync::{Edit, Method, PauseReason};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "onepaste", version, about = "OnePaste command line client")]
struct Cli {
    /// Directory that holds the local vault (default: ~/.onepaste)
    #[arg(long, global = true, env = "ONEPASTE_HOME")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Join a vault with a connection code
    Join {
        code: String,
        /// Name of this device
        #[arg(long)]
        name: Option<String>,
        /// Where to write the recovery kit of a new vault (default: print it to standard error)
        #[arg(long)]
        recovery_kit_out: Option<PathBuf>,
    },
    /// Print a connection code for another device
    Invite,
    /// Check that a file holds a valid recovery kit (exit code 3 if it does not)
    KitCheck { file: PathBuf },
    /// Add a text item read from standard input
    Add,
    /// List the local items
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print the content of an item to standard output
    Get { id: String },
    /// Change one field of an item
    #[command(group(ArgGroup::new("field").required(true).multiple(false).args(["favorite", "pinned", "note"])))]
    Set {
        id: String,
        #[arg(long)]
        favorite: Option<bool>,
        #[arg(long)]
        pinned: Option<bool>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Delete an item
    Rm { id: String },
    /// Upload queued changes, then pull once
    Sync,
    /// Stay running: listen for changes, pull and upload until interrupted
    Watch,
    /// Show the sync state
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List the devices of the vault
    Devices,
    /// Revoke a device
    Revoke { device_id: String },
    /// Forget the local copy and pull everything again (use after the server was rolled back)
    Resync,
    /// Test only: read an envelope from standard input and try to open it with a random wrong key
    #[cfg(feature = "fault-injection")]
    ProbeEnvelope,
}

fn data_dir(cli: &Cli) -> PathBuf {
    cli.data_dir.clone().unwrap_or_else(|| {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        home.join(".onepaste")
    })
}

fn env_passphrase() -> Option<String> {
    std::env::var("ONEPASTE_PASSPHRASE").ok().filter(|p| !p.is_empty())
}

fn prompt_passphrase() -> Result<String, ClientError> {
    rpassword::prompt_password("Passphrase: ").map_err(|e| ClientError::Other(e.to_string()))
}

fn open(cli: &Cli) -> Result<Session, ClientError> {
    let pass = match env_passphrase() {
        Some(p) => p,
        None => prompt_passphrase()?,
    };
    Session::open(&data_dir(cli), &pass)
}

/// Runs one sync. Paused states and an unreachable server become errors; stuck operations become exit code 6.
async fn sync_once(s: &mut Session) -> Result<u8, ClientError> {
    s.engine.request_pull();
    let report = s.run(false).await?;
    match report.end {
        RunEnd::Paused(r) => return Err(ClientError::Paused(r)),
        RunEnd::Waiting => return Err(ClientError::Transport("the server asked to retry later".into())),
        RunEnd::Idle => {}
    }
    eprintln!("synced: uploaded {}, pulled {}", report.pushed, report.pulled);
    let st = s.engine.status()?;
    if st.conflict + st.failed > 0 {
        eprintln!("{} queued change(s) are stuck ({} conflict, {} failed)", st.conflict + st.failed, st.conflict, st.failed);
        return Ok(6);
    }
    Ok(0)
}

fn first_line_text(v: &str) -> String {
    v.chars().take(60).collect()
}

async fn run(cli: Cli) -> Result<u8, ClientError> {
    match &cli.cmd {
        Cmd::Join { code, name, recovery_kit_out } => {
            let mut opts = JoinOptions {
                code: code.clone(),
                device_name: name.clone().unwrap_or_else(|| "onepaste-cli".to_string()),
                passphrase: env_passphrase(),
                recovery_kit_out: recovery_kit_out.clone(),
            };
            let res = match join(&data_dir(&cli), opts.clone()).await {
                Err(ClientError::PassphraseRequired) => {
                    opts.passphrase = Some(prompt_passphrase()?);
                    join(&data_dir(&cli), opts).await?
                }
                other => other?,
            };
            println!("Joined {} vault as device {}", if res.new_vault { "a new" } else { "the existing" }, res.device_id);
            if let Some(p) = res.generated_passphrase {
                println!("Passphrase (shown once, write it down): {p}");
            }
            if let Some(kit) = res.recovery_kit {
                eprintln!("Recovery kit (shown once, keep it safe): {kit}");
            } else if res.new_vault {
                eprintln!("The recovery kit was written to the file you named.");
            }
            Ok(0)
        }
        Cmd::Invite => {
            let s = open(&cli)?;
            println!("{}", s.invite().await?);
            Ok(0)
        }
        Cmd::KitCheck { file } => {
            let text = std::fs::read_to_string(file).map_err(|e| ClientError::Other(e.to_string()))?;
            onepaste_core::recovery::decode(text.trim())?;
            println!("valid");
            Ok(0)
        }
        Cmd::Add => {
            let mut raw = Vec::new();
            std::io::stdin().read_to_end(&mut raw).map_err(|e| ClientError::Other(e.to_string()))?;
            let text = String::from_utf8(raw).map_err(|_| ClientError::Other("the input is not valid UTF-8".into()))?;
            let mut s = open(&cli)?;
            let payload = ItemPayload { kind: "text".into(), content: Some(text), ..Default::default() };
            let id = s.engine.enqueue_create(now_ms(), &payload)?;
            onepaste_client::crash_point("after_enqueue");
            println!("{id}");
            let code = sync_once(&mut s).await.inspect_err(|_| eprintln!("the item is queued and will be uploaded by the next sync"))?;
            Ok(code)
        }
        Cmd::List { json } => {
            let mut s = open(&cli)?;
            let items = s.engine.list_items()?;
            if *json {
                let v: Vec<Value> = items
                    .iter()
                    .map(|i| json!({ "id": i.id, "seq": i.seq, "deleted": i.deleted, "decrypt_failed": i.decrypt_failed, "first_line": i.first_line, "favorite": i.favorite, "pinned": i.pinned, "note": i.note, "tags": i.tags, "queued": i.queued }))
                    .collect();
                println!("{}", Value::Array(v));
            } else {
                for i in &items {
                    let tag = if i.deleted { "[deleted]".to_string() } else if i.decrypt_failed { "[decrypt_failed]".to_string() } else { first_line_text(&i.first_line) };
                    println!("{}\t{}\t{}\t{}\t{}", i.id, i.seq, if i.favorite { "favorite" } else { "-" }, if i.pinned { "pinned" } else { "-" }, tag);
                }
            }
            Ok(0)
        }
        Cmd::Get { id } => {
            let mut s = open(&cli)?;
            let p = s.engine.get_item(id)?;
            std::io::stdout().write_all(p.content.unwrap_or_default().as_bytes()).map_err(|e| ClientError::Other(e.to_string()))?;
            Ok(0)
        }
        Cmd::Set { id, favorite, pinned, note } => {
            let edit = match (favorite, pinned, note) {
                (Some(v), None, None) => Edit::Favorite(*v),
                (None, Some(v), None) => Edit::Pinned(*v),
                (None, None, Some(v)) => Edit::Note(Some(v.clone())),
                _ => return Err(ClientError::Other("give exactly one of --favorite, --pinned, --note".into())),
            };
            let mut s = open(&cli)?;
            s.engine.enqueue_edit(now_ms(), id, edit)?;
            onepaste_client::crash_point("after_enqueue");
            sync_once(&mut s).await
        }
        Cmd::Rm { id } => {
            let mut s = open(&cli)?;
            s.engine.enqueue_delete(now_ms(), id)?;
            onepaste_client::crash_point("after_enqueue");
            sync_once(&mut s).await
        }
        Cmd::Sync => {
            let mut s = open(&cli)?;
            sync_once(&mut s).await
        }
        Cmd::Watch => {
            let mut s = open(&cli)?;
            eprintln!("watching; press Ctrl+C to stop");
            tokio::select! {
                r = s.watch() => r.map(|_| 0),
                _ = tokio::signal::ctrl_c() => Ok(0),
            }
        }
        Cmd::Status { json } => {
            let mut s = open(&cli)?;
            let st = s.engine.status()?;
            let paused = st.paused.map(|p: PauseReason| p.as_str()).unwrap_or("");
            if *json {
                println!("{}", json!({ "cursor": st.cursor, "max_seen_seq": st.max_seen_seq, "paused": paused, "refetching": st.refetching, "outbox": { "pending": st.pending, "conflict": st.conflict, "failed": st.failed } }));
            } else {
                println!("cursor: {}\nmax_seen_seq: {}\npaused: {}\noutbox: {} pending, {} conflict, {} failed", st.cursor, st.max_seen_seq, if paused.is_empty() { "no" } else { paused }, st.pending, st.conflict, st.failed);
            }
            Ok(0)
        }
        Cmd::Devices => {
            let s = open(&cli)?;
            let v = s.call(Method::Get, "/v1/devices", None).await?;
            for d in v.as_array().cloned().unwrap_or_default() {
                let id = d["device_id"].as_str().unwrap_or("");
                let name = d["sealed_name"]
                    .as_str()
                    .and_then(|n| B64.decode(n).ok())
                    .and_then(|raw| crypto::open(s.master_key(), &aad::device(id), &raw).ok())
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| "[unreadable]".to_string());
                let here = if id == s.info.device_id { " (this device)" } else { "" };
                println!("{id}\t{name}\t{}{here}", if d["revoked"].as_bool().unwrap_or(false) { "revoked" } else { "active" });
            }
            Ok(0)
        }
        Cmd::Revoke { device_id } => {
            let s = open(&cli)?;
            s.call(Method::Delete, &format!("/v1/devices/{device_id}"), None).await?;
            println!("Revoked {device_id}");
            Ok(0)
        }
        #[cfg(feature = "fault-injection")]
        Cmd::ProbeEnvelope => {
            let mut raw = String::new();
            std::io::stdin().read_to_string(&mut raw).map_err(|e| ClientError::Other(e.to_string()))?;
            let env: onepaste_core::envelope::Envelope = serde_json::from_str(raw.trim()).map_err(|_| ClientError::Other("not an envelope".into()))?;
            let wrong = crypto::Key::random()?;
            match onepaste_core::envelope::open_item(&wrong, &env) {
                Err(onepaste_core::Error::DecryptFailed) => {
                    println!("decrypt failed with a wrong key");
                    Ok(0)
                }
                Ok(_) => Err(ClientError::Other("an envelope opened with a random key".into())),
                Err(e) => Err(e.into()),
            }
        }
        Cmd::Resync => {
            let mut s = open(&cli)?;
            s.engine.resync()?;
            eprintln!("The local copy was cleared and everything is pulled again. Items that exist only on this device and were lost on the server are not uploaded again.");
            sync_once(&mut s).await
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    onepaste_client::disable_core_dumps();
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}
