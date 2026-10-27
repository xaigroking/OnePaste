use onepaste_server::config::Config;
use onepaste_server::{connect_code, Server};
use std::net::SocketAddr;
use std::path::PathBuf;

const USAGE: &str = "Usage: onepaste-server [connect-code] [--data-dir <dir>] [--listen <addr>] [--public-url <url>]\n\
Environment: ONEPASTE_DATA_DIR, ONEPASTE_LISTEN, ONEPASTE_PUBLIC_URL (command line arguments win)";

struct Cli {
    connect_code: bool,
    data_dir: Option<String>,
    listen: Option<String>,
    public_url: Option<String>,
}

fn parse_args() -> Result<Cli, String> {
    let mut cli = Cli { connect_code: false, data_dir: None, listen: None, public_url: None };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "connect-code" => cli.connect_code = true,
            "--data-dir" => cli.data_dir = Some(value("--data-dir")?),
            "--listen" => cli.listen = Some(value("--listen")?),
            "--public-url" => cli.public_url = Some(value("--public-url")?),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(cli)
}

fn build_config(cli: &Cli) -> Result<Config, String> {
    let pick = |flag: &Option<String>, var: &str| flag.clone().or_else(|| std::env::var(var).ok()).filter(|v| !v.is_empty());
    let data_dir = pick(&cli.data_dir, "ONEPASTE_DATA_DIR").unwrap_or_else(|| "./data".to_string());
    let mut cfg = Config::new(PathBuf::from(data_dir));
    if let Some(l) = pick(&cli.listen, "ONEPASTE_LISTEN") {
        cfg.listen = l.parse::<SocketAddr>().map_err(|_| format!("invalid listen address: {l}"))?;
    }
    cfg.public_url = pick(&cli.public_url, "ONEPASTE_PUBLIC_URL");
    Ok(cfg)
}

#[tokio::main]
async fn main() {
    let cli = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    #[allow(unused_mut)]
    let mut cfg = match build_config(&cli) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    #[cfg(feature = "fault-injection")]
    if let Some(n) = std::env::var("ONEPASTE_TOMBSTONE_RETENTION_SECS").ok().and_then(|v| v.parse().ok()) {
        cfg.tombstone_retention_secs = n;
    }
    tracing_subscriber::fmt().json().with_writer(std::io::stdout).init();

    if cli.connect_code {
        match connect_code(&cfg) {
            Ok(code) => println!("{code}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let server = match Server::start(cfg).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    tracing::info!(listen = %server.addr, fingerprint = %server.fingerprint, "started");
    let _ = tokio::signal::ctrl_c().await;
    server.shutdown().await;
}
