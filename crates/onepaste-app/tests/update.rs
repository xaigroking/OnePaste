//! Looking for updates: how versions are compared, what makes a manifest trustworthy, what a check does when something
//! is wrong, and what an update request tells the server.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_app::update::{self, check, default_channel, is_newer, verify, Channel, HttpsSource, Manifest, PlatformFile, Source, UpdateError};
use onepaste_app::*;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PLATFORM: &str = "windows-x86_64";

// ---- keys and signatures, in the form Tauri's signer uses: the base64 of a minisign file ------------------------------

struct Keys {
    public: String,
    secret: minisign::SecretKey,
    pk: minisign::PublicKey,
}

fn keys() -> Keys {
    let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Keys { public: B64.encode(kp.pk.to_box().unwrap().to_string()), secret: kp.sk, pk: kp.pk }
}

fn sign(k: &Keys, data: &[u8]) -> String {
    let sig = minisign::sign(Some(&k.pk), &k.secret, std::io::Cursor::new(data), Some("trusted"), Some("untrusted")).unwrap();
    B64.encode(sig.to_string())
}

fn manifest(version: &str, released_at: u64) -> Manifest {
    let mut platforms = BTreeMap::new();
    for platform in [PLATFORM, "darwin-universal", "linux-x86_64"] {
        platforms.insert(platform.to_string(), PlatformFile { url: format!("https://github.com/x/y/releases/download/v{version}/app.zip"), sha256: "00".repeat(32), size: 1 });
    }
    Manifest {
        schema: 1,
        version: version.into(),
        tag: format!("v{version}"),
        released_at,
        kind: "main".into(),
        notes: "<b>plain</b> text".into(),
        release_url: format!("https://github.com/x/y/releases/tag/v{version}"),
        platforms,
    }
}

// ---- a source that serves fixed answers ----------------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    answers: Mutex<HashMap<String, Result<Vec<u8>, UpdateError>>>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn put(&self, url: &str, body: Vec<u8>) {
        self.answers.lock().unwrap().insert(url.into(), Ok(body));
    }
    fn fail(&self, url: &str, e: UpdateError) {
        self.answers.lock().unwrap().insert(url.into(), Err(e));
    }
}

impl Source for Fake {
    fn get<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateError>> + Send + 'a>> {
        self.asked.lock().unwrap().push(url.to_string());
        let r = self.answers.lock().unwrap().get(url).cloned().unwrap_or(Err(UpdateError::Status(404)));
        Box::pin(async move { r })
    }
}

const LIST: &str = "https://example.test/releases";

/// Publishes a release in the fake: its entry in the list is returned, its manifest and signature are served.
fn release(fake: &Fake, k: &Keys, tag: &str, prerelease: bool, m: &Manifest) -> serde_json::Value {
    let bytes = serde_json::to_vec(m).unwrap();
    fake.put(&format!("https://example.test/{tag}/latest.json"), bytes.clone());
    fake.put(&format!("https://example.test/{tag}/latest.json.sig"), sign(k, &bytes).into_bytes());
    serde_json::json!({
        "tag_name": tag, "draft": false, "prerelease": prerelease,
        "assets": [
            {"name": "latest.json", "browser_download_url": format!("https://example.test/{tag}/latest.json")},
            {"name": "latest.json.sig", "browser_download_url": format!("https://example.test/{tag}/latest.json.sig")},
        ]
    })
}

fn serve_list(fake: &Fake, entries: Vec<serde_json::Value>) {
    fake.put(LIST, serde_json::to_vec(&entries).unwrap());
}

// ---- version comparison --------------------------------------------------------------------------------------------

#[test]
fn versions_follow_semver_and_pre_releases_with_the_same_numbers_follow_the_release_time() {
    assert!(is_newer("0.0.1", 0, "0.0.2", 0), "a higher number is newer");
    assert!(is_newer("0.0.1-main.99", 5, "0.0.2-main.1", 1), "numbers win over the pre-release part");
    assert!(is_newer("0.0.1-main.5", 5, "0.0.1", 1), "a release is newer than a pre-release of the same numbers");
    assert!(!is_newer("0.0.1", 5, "0.0.1-main.5", 9), "a pre-release is not newer than the release");
    // The two kinds of pre-release cannot be told apart by name: the time decides, either way round
    assert!(is_newer("0.0.1-beta.4", 100, "0.0.1-main.12", 200));
    assert!(!is_newer("0.0.1-main.12", 200, "0.0.1-beta.4", 100));
    // A string comparison would put main.9 after main.12; the times say the opposite
    assert!(is_newer("0.0.1-main.9", 100, "0.0.1-main.12", 200));
    assert!(!is_newer("0.0.1-main.12", 200, "0.0.1-main.9", 100));
    // The same release is never newer than itself, and nonsense is never newer
    assert!(!is_newer("0.0.1-main.12", 100, "v0.0.1-main.12", 999));
    assert!(!is_newer("0.0.1", 0, "not a version", 9));
}

#[test]
fn the_default_channel_includes_pre_releases_until_version_one() {
    assert_eq!(default_channel("0.0.1-main.4"), Channel::Preview);
    assert_eq!(default_channel("0.9.9"), Channel::Preview);
    assert_eq!(default_channel("1.0.0"), Channel::Stable);
    assert_eq!(default_channel("2.3.1-beta.1"), Channel::Stable);
}

// ---- signatures ----------------------------------------------------------------------------------------------------

#[test]
fn a_manifest_signature_passes_and_every_kind_of_tampering_fails() {
    let k = keys();
    let data = b"{\"schema\":1}".to_vec();
    let sig = sign(&k, &data);
    assert_eq!(verify(&data, &sig, &k.public), Ok(()));
    let mut changed = data.clone();
    changed[3] ^= 1;
    assert_eq!(verify(&changed, &sig, &k.public), Err(UpdateError::BadSignature), "one changed byte");
    assert_eq!(verify(&data, &sig, &keys().public), Err(UpdateError::BadSignature), "another key");
    assert_eq!(verify(&data, "", &k.public), Err(UpdateError::BadSignature), "an empty signature");
    assert_eq!(verify(&data, "   \n", &k.public), Err(UpdateError::BadSignature), "a blank signature");
    assert_eq!(verify(&data, "not base64 at all", &k.public), Err(UpdateError::BadSignature), "garbage");
    assert_eq!(verify(&data, &sig, "none"), Err(UpdateError::BadSignature), "no usable key");
}

// ---- the check -----------------------------------------------------------------------------------------------------

async fn run(fake: &Fake, k: &Keys, current: &str, built: u64, channel: Channel) -> Result<Option<UpdateInfo>, UpdateError> {
    check(fake, LIST, &k.public, current, built, channel, PLATFORM).await
}

#[tokio::test]
async fn the_channels_pick_different_releases() {
    let (fake, k) = (Fake::default(), keys());
    let entries = vec![
        release(&fake, &k, "v0.0.1-main.20", true, &manifest("0.0.1-main.20", 300)),
        release(&fake, &k, "v0.0.1-beta.5", true, &manifest("0.0.1-beta.5", 200)),
        release(&fake, &k, "v0.0.1-beta.4", true, &manifest("0.0.1-beta.4", 100)),
    ];
    serve_list(&fake, entries);
    let all = run(&fake, &k, "0.0.1-beta.3", 10, Channel::Preview).await.unwrap().unwrap();
    assert_eq!(all.version, "0.0.1-main.20", "the preview channel takes the newest of everything");
    let hand = run(&fake, &k, "0.0.1-beta.3", 10, Channel::Beta).await.unwrap().unwrap();
    assert_eq!(hand.version, "0.0.1-beta.5", "the beta channel leaves out the automatic pre-releases");
    assert_eq!(run(&fake, &k, "0.0.1-beta.3", 10, Channel::Stable).await.unwrap(), None, "there is no stable release");
}

#[tokio::test]
async fn nothing_is_offered_when_the_running_version_is_the_newest_or_newer() {
    let (fake, k) = (Fake::default(), keys());
    let entries = vec![release(&fake, &k, "v0.0.1-main.20", true, &manifest("0.0.1-main.20", 300))];
    serve_list(&fake, entries);
    assert_eq!(run(&fake, &k, "0.0.1-main.20", 250, Channel::Preview).await.unwrap(), None);
    assert_eq!(run(&fake, &k, "0.0.2", 1, Channel::Preview).await.unwrap(), None);
}

#[tokio::test]
async fn a_tampered_manifest_is_refused_even_if_it_looks_newer() {
    let (fake, k) = (Fake::default(), keys());
    let honest = manifest("0.0.1-main.20", 300);
    let entries = vec![release(&fake, &k, "v0.0.1-main.20", true, &honest)];
    serve_list(&fake, entries);
    // The attacker serves a manifest with another version and the signature of the real one
    let mut forged = honest.clone();
    forged.version = "9.9.9".into();
    fake.put("https://example.test/v0.0.1-main.20/latest.json", serde_json::to_vec(&forged).unwrap());
    assert_eq!(run(&fake, &k, "0.0.1-main.1", 1, Channel::Preview).await, Err(UpdateError::BadSignature));
    // A manifest signed by somebody else is refused as well
    let other = keys();
    let entries = vec![release(&fake, &other, "v0.0.1-main.21", true, &manifest("0.0.1-main.21", 400))];
    serve_list(&fake, entries);
    assert_eq!(run(&fake, &k, "0.0.1-main.1", 1, Channel::Preview).await, Err(UpdateError::BadSignature));
}

#[tokio::test]
async fn a_release_without_a_manifest_is_skipped_and_damage_is_reported() {
    let (fake, k) = (Fake::default(), keys());
    let without = serde_json::json!({"tag_name": "v0.0.1-main.30", "draft": false, "prerelease": true, "assets": []});
    let with = release(&fake, &k, "v0.0.1-main.20", true, &manifest("0.0.1-main.20", 300));
    serve_list(&fake, vec![without, with]);
    assert_eq!(run(&fake, &k, "0.0.1-main.1", 1, Channel::Preview).await.unwrap().unwrap().version, "0.0.1-main.20");
    fake.put(LIST, b"{ not json".to_vec());
    assert_eq!(run(&fake, &k, "0.0.1-main.1", 1, Channel::Preview).await, Err(UpdateError::Damaged));
    fake.fail(LIST, UpdateError::Unreachable("connect".into()));
    assert!(matches!(run(&fake, &k, "0.0.1-main.1", 1, Channel::Preview).await, Err(UpdateError::Unreachable(_))));
    assert_eq!(check(&fake, LIST, "none", "0.0.1", 1, Channel::Preview, PLATFORM).await, Err(UpdateError::NoKey));
}

// ---- inside the application ----------------------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("onepaste-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn app_with(fake: Arc<Fake>, k: &Keys, name: &str) -> App {
    app_with_dir(fake, k, temp_dir(name))
}

fn app_with_dir(fake: Arc<Fake>, k: &Keys, dir: PathBuf) -> App {
    let cfg = AppConfig {
        update_version: "0.0.1-main.1".into(),
        built_at: 1,
        update_source: Some(UpdateSetup { source: fake, releases_url: LIST.into(), public_key: k.public.clone() }),
        ..AppConfig::default()
    };
    App::new(dir, Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), cfg)
}

#[tokio::test]
async fn a_failed_background_check_is_silent_and_a_manual_one_says_why() {
    let (fake, k) = (Arc::new(Fake::default()), keys());
    fake.fail(LIST, UpdateError::Unreachable("connect".into()));
    let app = app_with(fake.clone(), &k, "silent");
    let told = Arc::new(Mutex::new(0));
    let t = told.clone();
    app.on_update_available(move |_| *t.lock().unwrap() += 1);
    assert_eq!(app.check_for_update(false).await.unwrap(), None, "no error and no update");
    assert_eq!(*told.lock().unwrap(), 0, "nobody is told");
    assert!(app.event_log().iter().any(|e| e.kind == "update_check_failed"), "it is noted in the event log");
    assert_eq!(app.check_for_update(true).await.unwrap_err().code(), "update_unreachable", "the user who asked sees the reason");
    // A damaged list and a forged manifest are handled the same way
    fake.put(LIST, b"{ broken".to_vec());
    assert_eq!(app.check_for_update(false).await.unwrap(), None);
    assert_eq!(app.check_for_update(true).await.unwrap_err().code(), "update_damaged");
}

#[tokio::test]
async fn skipping_a_version_hides_it_but_a_newer_one_is_still_offered() {
    let (fake, k) = (Arc::new(Fake::default()), keys());
    let entries = vec![release(&fake, &k, "v0.0.1-main.5", true, &manifest("0.0.1-main.5", 500))];
    serve_list(&fake, entries);
    let dir = temp_dir("skip");
    let app = app_with_dir(fake.clone(), &k, dir.clone());
    let told = Arc::new(Mutex::new(Vec::new()));
    let t = told.clone();
    app.on_update_available(move |i| t.lock().unwrap().push(i.version.clone()));
    assert_eq!(app.check_for_update(false).await.unwrap().unwrap().version, "0.0.1-main.5");
    app.skip_update("0.0.1-main.5").unwrap();
    assert_eq!(app.available_update(), None);
    assert_eq!(app.check_for_update(false).await.unwrap(), None, "the skipped version is not offered again");
    assert_eq!(app.check_for_update(true).await.unwrap().unwrap().version, "0.0.1-main.5", "a manual check still shows it");
    let entries = vec![release(&fake, &k, "v0.0.1-main.6", true, &manifest("0.0.1-main.6", 600)), release(&fake, &k, "v0.0.1-main.5", true, &manifest("0.0.1-main.5", 500))];
    serve_list(&fake, entries);
    assert_eq!(app.check_for_update(false).await.unwrap().unwrap().version, "0.0.1-main.6", "a newer version is offered");
    assert_eq!(*told.lock().unwrap(), vec!["0.0.1-main.5".to_string(), "0.0.1-main.6".to_string()]);
    // The choice survives a restart
    let again = App::new(dir.clone(), Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), AppConfig::default());
    assert_eq!(again.settings().skipped_update.as_deref(), Some("0.0.1-main.5"));
}

#[tokio::test]
async fn the_channel_setting_and_its_default() {
    let (fake, k) = (Arc::new(Fake::default()), keys());
    let app = app_with(fake, &k, "channel");
    assert_eq!(app.update_channel(), Channel::Preview, "0.x defaults to including pre-releases");
    let mut s = app.settings();
    s.update_channel = Some(Channel::Beta);
    app.set_settings(s).unwrap();
    assert_eq!(app.update_channel(), Channel::Beta);
    assert!(app.settings().check_updates, "automatic checks are on by default");
}

// ---- what the request says -----------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_update_request_carries_only_a_user_agent_with_the_version_and_the_usual_http_headers() {
    let (k, dir) = (keys(), temp_dir("tls"));
    let material = onepaste_server::tls::load_or_create(&dir).unwrap();
    let seen: Arc<Mutex<Vec<(String, Vec<(String, String)>)>>> = Arc::default();
    let manifest_json = serde_json::to_vec(&manifest("0.0.1-main.8", 800)).unwrap();
    let sig = sign(&k, &manifest_json);
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let base = format!("https://127.0.0.1:{port}");
    let list = serde_json::json!([{
        "tag_name": "v0.0.1-main.8", "draft": false, "prerelease": true,
        "assets": [
            {"name": "latest.json", "browser_download_url": format!("{base}/m")},
            {"name": "latest.json.sig", "browser_download_url": format!("{base}/s")},
        ]
    }])
    .to_string();
    let s2 = seen.clone();
    let app = axum::Router::new().fallback(move |uri: axum::http::Uri, headers: axum::http::HeaderMap| {
        let (seen, list, manifest_json, sig) = (s2.clone(), list.clone(), manifest_json.clone(), sig.clone());
        async move {
            seen.lock().unwrap().push((uri.path_and_query().map(|p| p.to_string()).unwrap_or_default(), headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect()));
            match uri.path() {
                "/releases" => list.into_bytes(),
                "/m" => manifest_json,
                _ => sig.into_bytes(),
            }
        }
    });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&material.cert_path, &material.key_path).await.unwrap();
    tokio::spawn(axum_server::bind_rustls(format!("127.0.0.1:{port}").parse::<std::net::SocketAddr>().unwrap(), tls).serve(app.into_make_service()));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let source = HttpsSource::pinned("0.0.1-main.1", &material.fingerprint);
    let found = check(&source, &format!("{base}/releases"), &k.public, "0.0.1-main.1", 1, Channel::Preview, PLATFORM).await.unwrap().unwrap();
    assert_eq!(found.version, "0.0.1-main.8");
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 3, "the list, the manifest and its signature");
    for (path, headers) in &requests {
        let names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        println!("request {path}: {headers:?}");
        for name in &names {
            assert!(["host", "user-agent", "accept"].contains(name), "an unexpected header {name} in a request for {path}");
        }
        let ua = headers.iter().find(|(k, _)| k == "user-agent").map(|(_, v)| v.as_str()).unwrap_or("");
        assert_eq!(ua, "OnePaste/0.0.1-main.1", "the user agent holds the product and its version, nothing else");
        assert!(!path.contains('?') || path.starts_with("/releases?"), "no query other than the list size: {path}");
    }
    let _ = update::PUBLIC_KEY;
}
