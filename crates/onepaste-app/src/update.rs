//! Looking for a newer version. A check is one plain HTTPS GET of the list of releases and, for the newest one that
//! fits the chosen channel, of its signed manifest. The manifest signature is checked with a public key that is part of
//! this program; an unsigned, altered or wrongly signed manifest is refused. Nothing identifying is sent.

use crate::error::AppError;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

/// The public key that update manifests must be signed with: the text Tauri's signer prints for the public key (a
/// base64 wrapped minisign key). The word `none` means that no key has been set up yet, so no update is ever offered.
pub const PUBLIC_KEY: &str = include_str!("../update_public_key.txt");

/// Where the releases are listed.
pub const RELEASES_URL: &str = "https://api.github.com/repos/xaigroking/OnePaste/releases?per_page=30";

/// Which releases are offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Only releases that are not marked as pre-releases.
    Stable,
    /// Every release, including the automatic pre-releases of the main branch.
    Preview,
    /// Releases that were made by hand (stable ones and named betas), without the automatic pre-releases.
    Beta,
}

/// The channel to use when the user has not chosen one: while the version is 0.x there is no stable release, so
/// pre-releases are included; from 1.0.0 on it is the stable channel.
pub fn default_channel(current_version: &str) -> Channel {
    match parse_core(current_version) {
        Some((0, _, _)) => Channel::Preview,
        _ => Channel::Stable,
    }
}

fn parse_core(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    let mut p = core.split('.');
    Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?))
}

fn is_pre(v: &str) -> bool {
    v.trim().trim_start_matches('v').contains('-')
}

/// Is `candidate` newer than `current`? SemVer decides: the numbers first, and a release is newer than a pre-release
/// of the same numbers. Two pre-releases of the same numbers (`0.0.1-main.12` and `0.0.1-beta.4`) cannot be told apart
/// by their names, so the time each was released decides; the times come from the signed manifest. Unreadable versions
/// are never newer.
pub fn is_newer(current: &str, current_released_at: u64, candidate: &str, candidate_released_at: u64) -> bool {
    if current.trim().trim_start_matches('v') == candidate.trim().trim_start_matches('v') {
        return false;
    }
    let (Some(a), Some(b)) = (parse_core(current), parse_core(candidate)) else { return false };
    if a != b {
        return b > a;
    }
    match (is_pre(current), is_pre(candidate)) {
        (true, false) => true,
        (false, true) => false,
        (false, false) => false,
        (true, true) => candidate_released_at > current_released_at,
    }
}

/// One platform's download in a manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformFile {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// The update manifest of one release (`latest.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    pub tag: String,
    /// Seconds since the epoch.
    pub released_at: u64,
    /// `stable`, `beta` or `main`.
    pub kind: String,
    /// A short plain text; it is shown as text and never as markup.
    pub notes: String,
    pub release_url: String,
    pub platforms: std::collections::BTreeMap<String, PlatformFile>,
}

/// What the interface shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub release_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error("this build has no update key, so updates are not offered")]
    NoKey,
    #[error("the update server could not be reached ({0})")]
    Unreachable(String),
    #[error("the update server answered {0}")]
    Status(u16),
    #[error("the list of releases or the manifest is damaged")]
    Damaged,
    #[error("the manifest signature is missing or wrong")]
    BadSignature,
}

impl UpdateError {
    pub fn code(&self) -> &'static str {
        match self {
            UpdateError::NoKey => "update_no_key",
            UpdateError::Unreachable(_) => "update_unreachable",
            UpdateError::Status(_) => "update_status",
            UpdateError::Damaged => "update_damaged",
            UpdateError::BadSignature => "update_bad_signature",
        }
    }
}

impl From<UpdateError> for AppError {
    fn from(e: UpdateError) -> Self {
        AppError::Update(e.code().to_string())
    }
}

/// Checks a manifest's signature. `public_key` and `signature` are in the form Tauri's signer produces: the base64 of a
/// minisign file. A missing, empty or wrong signature, or a different key, is `BadSignature`.
pub fn verify(manifest: &[u8], signature: &str, public_key: &str) -> Result<(), UpdateError> {
    let unwrap = |s: &str| -> Result<String, UpdateError> {
        let raw = B64.decode(s.trim()).map_err(|_| UpdateError::BadSignature)?;
        String::from_utf8(raw).map_err(|_| UpdateError::BadSignature)
    };
    if signature.trim().is_empty() {
        return Err(UpdateError::BadSignature);
    }
    let key = minisign_verify::PublicKey::decode(&unwrap(public_key)?).map_err(|_| UpdateError::BadSignature)?;
    let sig = minisign_verify::Signature::decode(&unwrap(signature)?).map_err(|_| UpdateError::BadSignature)?;
    key.verify(manifest, &sig, false).map_err(|_| UpdateError::BadSignature)
}

/// A release as the list of releases gives it.
#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseEntry {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<AssetEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AssetEntry {
    pub name: String,
    pub browser_download_url: String,
}

/// Where the data comes from. The real one is HTTPS; tests bring their own.
pub trait Source: Send + Sync {
    fn get<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateError>> + Send + 'a>>;
}

/// The real source: plain HTTPS GET requests with the system's trusted roots.
pub struct HttpsSource {
    client: reqwest::Client,
    user_agent: String,
}

impl HttpsSource {
    pub fn new(version: &str) -> Result<HttpsSource, UpdateError> {
        let client = onepaste_client::public::public_client(None).map_err(|e| UpdateError::Unreachable(e.to_string()))?;
        Ok(HttpsSource { client, user_agent: format!("OnePaste/{version}") })
    }

    /// For tests: trusts exactly one certificate.
    pub fn pinned(version: &str, fingerprint: &str) -> HttpsSource {
        HttpsSource {
            client: onepaste_client::public::public_client(Some(fingerprint)).expect("a pinned client can always be made"),
            user_agent: format!("OnePaste/{version}"),
        }
    }
}

impl Source for HttpsSource {
    fn get<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateError>> + Send + 'a>> {
        Box::pin(async move {
            let (status, body) = onepaste_client::public::get(&self.client, url, &self.user_agent, "application/json, application/octet-stream")
                .await
                .map_err(|e| UpdateError::Unreachable(e.to_string()))?;
            if status != 200 {
                return Err(UpdateError::Status(status));
            }
            Ok(body)
        })
    }
}

fn kind_of(tag: &str, prerelease: bool) -> &'static str {
    if tag.contains("-main.") {
        "main"
    } else if prerelease || tag.contains("-beta.") {
        "beta"
    } else {
        "stable"
    }
}

fn fits(channel: Channel, e: &ReleaseEntry) -> bool {
    if e.draft {
        return false;
    }
    match channel {
        Channel::Stable => !e.prerelease,
        Channel::Preview => true,
        Channel::Beta => kind_of(&e.tag_name, e.prerelease) != "main",
    }
}

/// How many releases are looked at before giving up (a release without a manifest is skipped).
const MAX_CANDIDATES: usize = 5;

/// Looks for a version newer than `current` on the channel. `Ok(None)` means there is none. The newest releases that
/// fit the channel and have a signed manifest are read in the order of the list; the first one decides.
pub async fn check(
    source: &dyn Source,
    releases_url: &str,
    public_key: &str,
    current: &str,
    current_released_at: u64,
    channel: Channel,
    platform: &str,
) -> Result<Option<UpdateInfo>, UpdateError> {
    if public_key.trim() == "none" || public_key.trim().is_empty() {
        return Err(UpdateError::NoKey);
    }
    let list = source.get(releases_url).await?;
    let releases: Vec<ReleaseEntry> = serde_json::from_slice(&list).map_err(|_| UpdateError::Damaged)?;
    let mut tried = 0;
    for release in releases.iter().filter(|r| fits(channel, r)) {
        let find = |name: &str| release.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
        let (Some(manifest_url), Some(sig_url)) = (find("latest.json"), find("latest.json.sig")) else { continue };
        tried += 1;
        if tried > MAX_CANDIDATES {
            break;
        }
        let manifest_bytes = source.get(&manifest_url).await?;
        let signature = String::from_utf8(source.get(&sig_url).await?).map_err(|_| UpdateError::BadSignature)?;
        verify(&manifest_bytes, &signature, public_key)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|_| UpdateError::Damaged)?;
        if manifest.schema != 1 || !manifest.release_url.starts_with("https://github.com/") || !manifest.platforms.contains_key(platform) {
            continue;
        }
        return Ok(is_newer(current, current_released_at, &manifest.version, manifest.released_at).then(|| UpdateInfo {
            version: manifest.version,
            notes: manifest.notes,
            release_url: manifest.release_url,
        }));
    }
    Ok(None)
}

/// The key of this computer's platform in a manifest.
pub fn platform_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        "darwin-universal"
    } else {
        "linux-x86_64"
    }
}
