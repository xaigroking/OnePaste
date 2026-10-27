use onepaste_server::config::Config;
use onepaste_server::{connect_code, Server};
use std::net::SocketAddr;
use std::path::PathBuf;

const USAGE: &str = "Usage: onepaste-server [connect-code | healthcheck] [--data-dir <dir>] [--listen <addr>] [--public-url <url>] [--version]\n\
Environment: ONEPASTE_DATA_DIR, ONEPASTE_LISTEN, ONEPASTE_PUBLIC_URL (command line arguments win)\n\
RUST_LOG=<trace|debug|info|warn|error> sets the log level (default: info)";

struct Cli {
    connect_code: bool,
    healthcheck: bool,
    data_dir: Option<String>,
    listen: Option<String>,
    public_url: Option<String>,
}

fn parse_args() -> Result<Cli, String> {
    let mut cli = Cli { connect_code: false, healthcheck: false, data_dir: None, listen: None, public_url: None };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "connect-code" => cli.connect_code = true,
            "healthcheck" => cli.healthcheck = true,
            "--data-dir" => cli.data_dir = Some(value("--data-dir")?),
            "--listen" => cli.listen = Some(value("--listen")?),
            "--public-url" => cli.public_url = Some(value("--public-url")?),
            "-V" | "--version" => {
                println!("onepaste-server {}", env!("ONEPASTE_VERSION_LONG"));
                std::process::exit(0);
            }
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
    onepaste_server::harden::disable_core_dumps();
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
    // For a container health check: succeeds if something accepts connections on the listen port of this server
    if cli.healthcheck {
        let target = SocketAddr::from(([127, 0, 0, 1], cfg.listen.port()));
        let up = std::net::TcpStream::connect_timeout(&target, std::time::Duration::from_secs(2)).is_ok();
        std::process::exit(if up { 0 } else { 1 });
    }
    #[cfg(feature = "fault-injection")]
    if let Some(n) = std::env::var("ONEPASTE_TOMBSTONE_RETENTION_SECS").ok().and_then(|v| v.parse().ok()) {
        cfg.tombstone_retention_secs = n;
    }
    let level = std::env::var("RUST_LOG").ok().and_then(|v| v.trim().to_ascii_lowercase().parse::<tracing::Level>().ok()).unwrap_or(tracing::Level::INFO);
    tracing_subscriber::fmt().json().with_max_level(level).with_writer(std::io::stdout).init();

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
    wait_for_stop().await;
    server.shutdown().await;
}

/// Waits for Ctrl-C or, on Unix, for the termination signal that `docker stop` and service managers send. As the first
/// process of a container, a program that ignores that signal is killed after a timeout instead of closing its database.
async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
                return;
            }
            Err(_) => {}
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
