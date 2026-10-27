//! Test harness: the whole application logic with a fake clipboard and an in-memory secret store, driven by text
//! commands on standard input. Each command prints its result and then a line `END`. Only built with the
//! `test-harness` feature; a release build does not contain it.
//!
//! Commands: join <code> <passphrase> <name> | copy <text> | copy-image | copy-sensitive <text> | sync |
//! list | search <query> | click <n> | clipboard | status | flag <n> favorite|pinned true|false | note <n> <text> |
//! rm <n> | quit

use onepaste_app::*;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

fn show(items: &[ItemView]) -> String {
    let mut out = String::new();
    for (n, i) in items.iter().enumerate() {
        out.push_str(&format!("{}\t{}\t{}\t{}\t{}\t{}\t{}x{}\t{}\n", n + 1, i.item_id, i.kind, i.preview, if i.favorite { "favorite" } else { "-" }, if i.pinned { "pinned" } else { "-" }, i.width, i.height, i.size));
    }
    out
}

fn err(e: AppError) -> String {
    format!("error {}\n", e.code())
}

#[tokio::main]
async fn main() {
    let dir = match std::env::args().nth(1) {
        Some(d) => std::path::PathBuf::from(d),
        None => {
            eprintln!("usage: onepaste-app-harness <data-directory>");
            std::process::exit(2);
        }
    };
    let clipboard = Arc::new(FakeClipboard::new());
    let secrets = Arc::new(MemorySecretStore::new());
    let app = App::new(dir, secrets, clipboard.clone(), AppConfig { listen_for_changes: false, poll_interval: Duration::from_secs(3600), background_sync: false });
    let mut last: Vec<ItemView> = Vec::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let nth = |s: &str| -> Option<String> { s.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).and_then(|n| last.get(n.wrapping_sub(1))).map(|i| i.item_id.clone()) };
        let out = match cmd {
            "join" => {
                let mut p = rest.splitn(3, ' ');
                let (code, pass, name) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or("harness"));
                match app.join(code, pass, name).await {
                    Ok(()) => "ok\n".to_string(),
                    Err(e) => err(e),
                }
            }
            "copy" => {
                clipboard.user_copies(Content::Text(rest.to_string()));
                tokio::time::sleep(Duration::from_millis(400)).await;
                "ok\n".to_string()
            }
            "copy-sensitive" => {
                clipboard.user_copies_sensitive(Content::Text(rest.to_string()));
                tokio::time::sleep(Duration::from_millis(200)).await;
                "ok\n".to_string()
            }
            "copy-image" => {
                let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
                png.extend(std::iter::repeat(9u8).take(500));
                clipboard.user_copies(Content::Image { png, width: 4, height: 3 });
                tokio::time::sleep(Duration::from_millis(400)).await;
                "ok\n".to_string()
            }
            "sync" => match app.sync_now().await {
                Ok(()) => {
                    // The own-write report of the clipboard monitor arrives on another task
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    "ok\n".to_string()
                }
                Err(e) => err(e),
            },
            "list" | "search" => {
                let q = if cmd == "list" { "" } else { rest };
                match app.search(q, Filter::All, 50, 0).await {
                    Ok(items) => {
                        let s = show(&items);
                        last = items;
                        s
                    }
                    Err(e) => err(e),
                }
            }
            "click" => match nth(rest) {
                Some(id) => app.copy(&id).await.map(|_| "ok\n".to_string()).unwrap_or_else(err),
                None => "error no_such_row\n".to_string(),
            },
            "clipboard" => match clipboard.current() {
                Some(Content::Text(t)) => format!("text {t}\n"),
                Some(Content::Image { width, height, .. }) => format!("image {width}x{height}\n"),
                None => "empty\n".to_string(),
            },
            "status" => {
                let st = app.status().await;
                format!("{}\n", serde_json::to_string(&st).unwrap_or_default())
            }
            "flag" => {
                let mut p = rest.split_whitespace();
                let (n, what, value) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next() == Some("true"));
                match nth(n) {
                    Some(id) => {
                        let (f, pin) = if what == "favorite" { (Some(value), None) } else { (None, Some(value)) };
                        app.set_flags(&id, f, pin, None).await.map(|_| "ok\n".to_string()).unwrap_or_else(err)
                    }
                    None => "error no_such_row\n".to_string(),
                }
            }
            "note" => {
                let (n, text) = rest.split_once(' ').unwrap_or((rest, ""));
                match nth(n) {
                    Some(id) => app.set_flags(&id, None, None, Some(text.to_string())).await.map(|_| "ok\n".to_string()).unwrap_or_else(err),
                    None => "error no_such_row\n".to_string(),
                }
            }
            "rm" => match nth(rest) {
                Some(id) => app.delete(&id).await.map(|_| "ok\n".to_string()).unwrap_or_else(err),
                None => "error no_such_row\n".to_string(),
            },
            "quit" => break,
            _ => "error unknown_command\n".to_string(),
        };
        let _ = write!(stdout, "{out}END\n");
        let _ = stdout.flush();
    }
    app.shutdown();
}
