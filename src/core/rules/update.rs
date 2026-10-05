//! Cache entries are authenticated on every load, including after elevation or a restart.
use super::{RuleBundle, RuleSnapshot, CAPABILITIES, MAX_PACKAGE, SCHEMA};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const CHECK_INTERVAL: u64 = 24 * 60 * 60;
pub const CHANNEL: &str =
    "https://github.com/meimingqi222/quick-cleaner/releases/download/rules-stable";
static UPDATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub sequence: u64,
    pub sha256: String,
    pub schema: u32,
    pub minimum_app_version: String,
    pub required: Vec<String>,
    pub key_id: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    current: u64,
    previous: u64,
    watermark: u64,
}

pub fn trusted_keys() -> BTreeMap<String, String> {
    serde_json::from_str(include_str!("../../../rules/trusted-keys.json"))
        .expect("valid trusted key registry")
}
pub fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], String> {
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid hex encoding".into());
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| "Invalid hex")?;
    }
    Ok(bytes)
}

/// Generates a fresh Ed25519 rule-signing keypair as `(private_hex, public_hex)`. This function
/// never writes the private key to disk: the operator captures it once into the CI secret
/// `QC_RULE_PRIVATE_KEY` and commits the public key into `rules/trusted-keys.json`.
pub fn generate_signing_key() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|error| error.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    Ok((hex(&seed), hex(&key.verifying_key().to_bytes())))
}
pub fn verify(
    manifest: &[u8],
    signature: &[u8],
    package: &[u8],
    keys: &BTreeMap<String, String>,
    watermark: u64,
) -> Result<RuleBundle, String> {
    if manifest.len() > 64 * 1024 || package.len() > MAX_PACKAGE {
        return Err("Rule update exceeds size limit".into());
    }
    let header: Manifest = serde_json::from_slice(manifest).map_err(|e| e.to_string())?;
    let key = keys.get(&header.key_id).ok_or("Unknown rule signing key")?;
    let key = VerifyingKey::from_bytes(&decode_hex(key)?).map_err(|e| e.to_string())?;
    let signature = Signature::from_slice(signature).map_err(|e| e.to_string())?;
    key.verify_strict(manifest, &signature)
        .map_err(|_| "Invalid rule signature")?;
    if header.minimum_app_version.split('.').count() != 3
        || header
            .minimum_app_version
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Invalid minimum application version".into());
    }
    if header.schema != SCHEMA
        || header.sequence < watermark
        || header.sequence == 0
        || crate::core::updater::is_version_newer(
            &header.minimum_app_version,
            env!("CARGO_PKG_VERSION"),
        )
        || header
            .required
            .iter()
            .any(|c| !CAPABILITIES.contains(&c.as_str()))
    {
        return Err("Incompatible or replayed rule update".into());
    }
    if header.sha256 != format!("{:x}", Sha256::digest(package)) {
        return Err("Rule package digest mismatch".into());
    }
    let bundle = RuleBundle::parse(package)?;
    if bundle.sequence != header.sequence
        || bundle
            .rules
            .iter()
            .flat_map(|r| &r.required)
            .any(|c| !header.required.contains(c))
    {
        return Err("Rule manifest/package mismatch".into());
    }
    Ok(bundle)
}
fn cache_root() -> Option<PathBuf> {
    crate::core::settings::config_dir().map(|p| p.join("rules"))
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !md.is_file() || super::facts::is_link(&md) || md.len() > limit as u64 {
        return Err("Invalid rule cache file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Rule cache limit".into());
    }
    Ok(bytes)
}
fn state_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let directory = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot read rule state: {error}")),
    };
    check_directory(root)?;
    let mut files = Vec::new();
    for (count, entry) in directory.enumerate() {
        if count >= 1024 {
            return Err("Rule cache entry limit".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_name().to_string_lossy().starts_with("state-")
            && entry.path().extension().is_some_and(|e| e == "json")
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}
fn state(root: &Path) -> Result<State, String> {
    let files = state_files(root)?;
    if files.is_empty() {
        return Ok(State::default());
    }
    let mut latest = None;
    let mut watermark = 0;
    for path in files.into_iter().rev() {
        let Ok(bytes) = read_bounded(&path, 4096) else {
            return Err("Unreadable rule state".into());
        };
        let Ok(candidate) = serde_json::from_slice::<State>(&bytes) else {
            continue;
        };
        if candidate.current > candidate.watermark || candidate.previous > candidate.watermark {
            return Err("Invalid rule state watermark".into());
        }
        watermark = watermark.max(candidate.watermark);
        latest.get_or_insert(candidate);
    }
    let mut latest = latest.ok_or("No valid rule state; refusing to reset replay protection")?;
    latest.watermark = watermark;
    Ok(latest)
}
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}
fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn write_state(root: &Path, state: &State) -> Result<(), String> {
    // Rename to a new immutable filename works atomically on both Windows and macOS.
    let generation = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let target = root.join(format!("state-{generation:039}.json"));
    let temporary = target.with_extension("tmp");
    check_directory(root)?;
    write_synced(
        &temporary,
        &serde_json::to_vec(state).map_err(|e| e.to_string())?,
    )?;
    std::fs::rename(temporary, target).map_err(|e| e.to_string())?;
    sync_directory(root)?;
    let old = match state_files(root) {
        Ok(files) => files,
        Err(error) => {
            crate::log!("Rule state retention: {error}");
            return Ok(());
        }
    };
    let remove = old.len().saturating_sub(2);
    for path in old.into_iter().take(remove) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}
fn load_with_keys(
    root: &Path,
    sequence: u64,
    keys: &BTreeMap<String, String>,
) -> Result<RuleBundle, String> {
    let dir = root.join(sequence.to_string());
    check_directory(&dir)?;
    let bundle = verify(
        &read_bounded(&dir.join("manifest.json"), 64 * 1024)?,
        &read_bounded(&dir.join("manifest.sig"), 64)?,
        &read_bounded(&dir.join("rules.json"), MAX_PACKAGE)?,
        keys,
        0,
    )?;
    if bundle.sequence != sequence {
        return Err("Rule cache sequence mismatch".into());
    }
    Ok(bundle)
}
fn check_directory(path: &Path) -> Result<(), String> {
    for p in path.ancestors() {
        let md = std::fs::symlink_metadata(p).map_err(|e| e.to_string())?;
        if !md.is_dir() || super::facts::is_link(&md) {
            return Err("Redirected rule cache".into());
        }
    }
    Ok(())
}
pub(super) fn load_cached() -> Option<Arc<RuleSnapshot>> {
    let root = cache_root()?;
    load_selected(&root, &trusted_keys())
        .ok()
        .map(|bundle| Arc::new(RuleSnapshot { bundle }))
}
fn load_selected(root: &Path, keys: &BTreeMap<String, String>) -> Result<RuleBundle, String> {
    let index = state(root)?;
    if index.current == 0 {
        return Ok(super::embedded().bundle.clone());
    }
    [index.current, index.previous]
        .into_iter()
        .filter(|s| *s != 0)
        .find_map(|s| load_with_keys(root, s, keys).ok())
        .ok_or_else(|| "No authenticated cached rules".into())
}
fn fetch(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .set("User-Agent", "QuickCleaner-Rules")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Rule download exceeds size limit".into());
    }
    Ok(bytes)
}
pub fn check_and_update() -> Result<u64, String> {
    let _lock = UPDATE_LOCK.lock().map_err(|_| "Rule update lock")?;
    if trusted_keys().is_empty() {
        return Err("Rule signing key has not been provisioned".into());
    }
    let keys = trusted_keys();
    let root = cache_root().ok_or("Missing real-user configuration directory")?;
    let bundle = match install_channel(CHANNEL, &root, &keys, &fetch)? {
        Some(bundle) => bundle,
        None => return Ok(super::snapshot().bundle.sequence),
    };
    let sequence = bundle.sequence;
    super::activate(bundle);
    Ok(sequence)
}

/// Rule-channel transport: `fetch(url, byte_limit)` returns bounded response bytes.
type Transport = dyn Fn(&str, usize) -> Result<Vec<u8>, String>;

/// Fetches, verifies and stages a channel update without activating it. Returns `Ok(None)`
/// when the channel sequence is not newer than the accepted watermark.
fn install_channel(
    channel: &str,
    root: &Path,
    keys: &BTreeMap<String, String>,
    fetch: &Transport,
) -> Result<Option<RuleBundle>, String> {
    let index = state(root)?;
    let manifest = fetch(&format!("{channel}/manifest.json"), 64 * 1024)?;
    let signature = fetch(&format!("{channel}/manifest.sig"), 64)?;
    let header: Manifest = serde_json::from_slice(&manifest).map_err(|e| e.to_string())?;
    // Authenticate the channel before allowing it to choose a package URL.
    let key = VerifyingKey::from_bytes(&decode_hex(
        keys.get(&header.key_id).ok_or("Unknown rule signing key")?,
    )?)
    .map_err(|e| e.to_string())?;
    key.verify_strict(
        &manifest,
        &Signature::from_slice(&signature).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "Invalid rule signature")?;
    if header.sequence <= index.watermark {
        return Ok(None);
    }
    let url = format!(
        "https://github.com/meimingqi222/quick-cleaner/releases/download/rules-v{}/rules.json",
        header.sequence
    );
    let package = fetch(&url, MAX_PACKAGE)?;
    install_at(root, &manifest, &signature, &package, keys).map(Some)
}
fn store_package(
    root: &Path,
    manifest: &[u8],
    signature: &[u8],
    package: &[u8],
    keys: &BTreeMap<String, String>,
    bundle: &RuleBundle,
) -> Result<(), String> {
    create_cache_directory(root)?;
    let generation = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    // Each retry has its own unpublished directory. Interrupted downloads never block later attempts.
    let staging = root.join(format!("{}-{generation}.staging", bundle.sequence));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let publish = || -> Result<(), String> {
        for (name, bytes) in [
            ("manifest.json", manifest),
            ("manifest.sig", signature),
            ("rules.json", package),
        ] {
            write_synced(&staging.join(name), bytes)?;
        }
        sync_directory(&staging)?;
        let destination = root.join(bundle.sequence.to_string());
        match std::fs::symlink_metadata(&destination) {
            Ok(_) => {
                // A previous attempt may have published the immutable package but failed to publish its pointer.
                let existing = load_with_keys(root, bundle.sequence, keys)?;
                if serde_json::to_vec(&existing).map_err(|e| e.to_string())?
                    != serde_json::to_vec(bundle).map_err(|e| e.to_string())?
                {
                    return Err("Immutable rule package conflict".into());
                }
                remove_package_files(&staging)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::rename(&staging, destination).map_err(|e| e.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
        sync_directory(root)?;
        Ok(())
    };
    let result = publish();
    if result.is_err() {
        // Only this attempt's exact files may be reclaimed; unknown entries survive.
        let _ = remove_package_files(&staging);
    }
    result
}

fn create_cache_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => check_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_cache_directory(path.parent().ok_or("Missing cache ancestor")?)?;
            std::fs::create_dir(path).map_err(|e| e.to_string())?;
            check_directory(path)
        }
        Err(error) => Err(error.to_string()),
    }
}
fn install_at(
    root: &Path,
    manifest: &[u8],
    signature: &[u8],
    package: &[u8],
    keys: &BTreeMap<String, String>,
) -> Result<RuleBundle, String> {
    let index = state(root)?;
    let bundle = verify(manifest, signature, package, keys, index.watermark)?;
    if bundle.sequence <= index.watermark {
        return Err("Rule sequence already accepted".into());
    }
    store_package(root, manifest, signature, package, keys, &bundle)?;
    let accepted = State {
        current: bundle.sequence,
        previous: index.current,
        watermark: bundle.sequence,
    };
    write_state(root, &accepted)?;
    // Retention is best effort after the durable commit; failure must not undo accepted state.
    if let Err(error) = prune_packages(root, &accepted, keys) {
        crate::log!("Rule retention: {error}");
    }
    Ok(bundle)
}
fn remove_package_files(directory: &Path) -> Result<(), String> {
    check_directory(directory)?;
    let entries = std::fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if entries.len() > 3
        || entries.iter().any(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some("manifest.json" | "manifest.sig" | "rules.json")
            )
        })
    {
        return Err("Unknown rule cache contents".into());
    }
    for entry in &entries {
        let md = entry.metadata().map_err(|e| e.to_string())?;
        if !md.is_file()
            || super::facts::is_link(
                &std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?,
            )
        {
            return Err("Redirected rule cache entry".into());
        }
    }
    for entry in entries {
        std::fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
    }
    std::fs::remove_dir(directory).map_err(|e| e.to_string())
}
fn prune_packages(
    root: &Path,
    index: &State,
    keys: &BTreeMap<String, String>,
) -> Result<(), String> {
    check_directory(root)?;
    for (count, entry) in std::fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if count >= 1024 {
            return Err("Rule cache entry limit".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(sequence) = name.parse::<u64>() else {
            continue;
        };
        if sequence == 0
            || sequence == index.current
            || sequence == index.previous
            || sequence.to_string() != name
        {
            continue;
        }
        if load_with_keys(root, sequence, keys).is_ok() {
            remove_package_files(&entry.path())?;
        }
    }
    Ok(())
}
pub fn rollback() -> Result<u64, String> {
    let _lock = UPDATE_LOCK.lock().map_err(|_| "Rule update lock")?;
    let root = cache_root().ok_or("Missing rule cache")?;
    let bundle = rollback_at(&root, &trusted_keys())?;
    let sequence = bundle.sequence;
    super::activate(bundle);
    Ok(sequence)
}
fn rollback_at(root: &Path, keys: &BTreeMap<String, String>) -> Result<RuleBundle, String> {
    let mut index = state(root)?;
    let bundle = if index.previous == 0 {
        super::embedded().bundle.clone()
    } else {
        load_with_keys(root, index.previous, keys)?
    };
    std::mem::swap(&mut index.current, &mut index.previous);
    write_state(root, &index)?;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    fn signed_bundle(bundle: &RuleBundle) -> (Vec<u8>, Vec<u8>, Vec<u8>, BTreeMap<String, String>) {
        let key = SigningKey::from_bytes(&[37; 32]);
        let package = serde_json::to_vec(bundle).unwrap();
        let manifest = serde_json::to_vec(&Manifest {
            sequence: bundle.sequence,
            sha256: format!("{:x}", Sha256::digest(&package)),
            schema: SCHEMA,
            minimum_app_version: env!("CARGO_PKG_VERSION").into(),
            required: CAPABILITIES.iter().map(|s| s.to_string()).collect(),
            key_id: "fixture".into(),
        })
        .unwrap();
        let signature = key.sign(&manifest).to_bytes().to_vec();
        let keys = BTreeMap::from([(
            "fixture".into(),
            key.verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )]);
        (manifest, signature, package, keys)
    }
    fn signed(sequence: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>, BTreeMap<String, String>) {
        let mut bundle = super::super::embedded().bundle.clone();
        bundle.sequence = sequence;
        signed_bundle(&bundle)
    }
    #[test]
    fn update_authenticates_entire_bundle_and_rejects_replay() {
        let (manifest, mut signature, mut package, keys) = signed(10);
        assert_eq!(
            verify(&manifest, &signature, &package, &keys, 9)
                .unwrap()
                .sequence,
            10
        );
        assert!(verify(&manifest, &signature, &package, &keys, 11).is_err());
        package.push(b' ');
        assert!(verify(&manifest, &signature, &package, &keys, 9).is_err());
        package.pop();
        signature[0] ^= 1;
        assert!(verify(&manifest, &signature, &package, &keys, 9).is_err());
        assert!(verify(&manifest, &signature, &package, &BTreeMap::new(), 9).is_err());
    }
    #[test]
    fn untrusted_hex_never_panics_on_unicode() {
        assert!(decode_hex::<2>("€a").is_err());
        assert!(decode_hex::<2>("zzzz").is_err());
        assert_eq!(decode_hex::<2>("abcd").unwrap(), [0xab, 0xcd]);
    }
    #[test]
    fn generated_signing_key_is_fresh_and_verifies_against_its_public_key() {
        let (private, public) = super::generate_signing_key().unwrap();
        assert_eq!(private.len(), 64);
        assert_eq!(public.len(), 64);
        let key = SigningKey::from_bytes(&decode_hex::<32>(&private).unwrap());
        let derived: String = key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            derived, public,
            "printed public key must match the private key"
        );
        let keys = BTreeMap::from([("fixture".to_string(), public.clone())]);
        let mut bundle = super::super::embedded().bundle.clone();
        bundle.sequence = 10;
        let package = serde_json::to_vec(&bundle).unwrap();
        let manifest = serde_json::to_vec(&Manifest {
            sequence: 10,
            sha256: format!("{:x}", Sha256::digest(&package)),
            schema: SCHEMA,
            minimum_app_version: env!("CARGO_PKG_VERSION").into(),
            required: CAPABILITIES.iter().map(|s| s.to_string()).collect(),
            key_id: "fixture".into(),
        })
        .unwrap();
        let signature = key.sign(&manifest).to_bytes().to_vec();
        assert_eq!(
            verify(&manifest, &signature, &package, &keys, 0)
                .unwrap()
                .sequence,
            10
        );
        // A package signed by the fixture key must not verify under the generated key.
        let (other_manifest, other_signature, other_package, _) = signed(10);
        assert!(verify(&other_manifest, &other_signature, &other_package, &keys, 0).is_err());
        assert_ne!(
            super::generate_signing_key().unwrap().0,
            private,
            "each generation must use fresh entropy"
        );
    }
    fn sign_header(header: &Manifest) -> (Vec<u8>, Vec<u8>, BTreeMap<String, String>) {
        let key = SigningKey::from_bytes(&[37; 32]);
        let manifest = serde_json::to_vec(header).unwrap();
        let signature = key.sign(&manifest).to_bytes().to_vec();
        let keys = BTreeMap::from([(
            "fixture".into(),
            key.verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )]);
        (manifest, signature, keys)
    }
    #[test]
    fn verify_rejects_incompatible_manifest_and_package_drift() {
        let (manifest, _, package, _) = signed(10);
        let header: Manifest = serde_json::from_slice(&manifest).unwrap();
        let reject = |header: Manifest, package: &[u8]| {
            let (manifest, signature, keys) = sign_header(&header);
            assert!(verify(&manifest, &signature, package, &keys, 0).is_err());
        };
        let mut too_new = header.clone();
        too_new.minimum_app_version = "999.0.0".into();
        reject(too_new, &package);
        let mut malformed_version = header.clone();
        malformed_version.minimum_app_version = "1.2".into();
        reject(malformed_version, &package);
        let mut schema = header.clone();
        schema.schema = SCHEMA + 1;
        reject(schema, &package);
        let mut replay = header.clone();
        replay.sequence = 0;
        reject(replay, &package);
        let mut unknown = header.clone();
        unknown.required.push("shell".into());
        reject(unknown, &package);

        let mut undeclared = header.clone();
        undeclared
            .required
            .retain(|capability| capability != "file");
        reject(undeclared, &package);

        let mut bundle: super::super::RuleBundle = serde_json::from_slice(&package).unwrap();
        bundle.sequence = 11;
        let drifted = serde_json::to_vec(&bundle).unwrap();
        let mut drifted_header = header;
        drifted_header.sha256 = format!("{:x}", Sha256::digest(&drifted));
        reject(drifted_header, &drifted);
    }
    #[test]
    fn cache_file_over_limit_or_not_a_file_is_rejected() {
        let root = crate::core::testing::fixture("rule_cache_reject");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("rules.json");
        std::fs::write(&file, vec![b'x'; 32]).unwrap();
        assert!(read_bounded(&file, 8).is_err());
        assert!(read_bounded(&root, 100).is_err());
        assert!(check_directory(&file).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn atomic_state_keeps_previous_and_watermark() {
        let root = crate::core::testing::fixture("rule_state");
        std::fs::create_dir_all(&root).unwrap();
        write_state(
            &root,
            &State {
                current: 7,
                previous: 6,
                watermark: 7,
            },
        )
        .unwrap();
        std::fs::write(
            root.join("state-999999999999999999999999999999999999999.json"),
            b"interrupted",
        )
        .unwrap();
        let s = state(&root).unwrap();
        assert_eq!((s.current, s.previous, s.watermark), (7, 6, 7));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_state_never_resets_replay_protection() {
        let root = crate::core::testing::fixture("rule_corrupt_state");
        std::fs::write(root.join("state-1.json"), b"interrupted").unwrap();
        assert!(state(&root).is_err());
        let (manifest, signature, package, keys) = signed(10);
        assert!(install_at(&root, &manifest, &signature, &package, &keys).is_err());
        assert!(!root.join("10").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_publication_retries_and_corrupt_updates_leave_active_state() {
        let root = crate::core::testing::fixture("rule_interrupted_install");
        let (manifest, signature, package, keys) = signed(10);
        let bundle = verify(&manifest, &signature, &package, &keys, 0).unwrap();
        store_package(&root, &manifest, &signature, &package, &keys, &bundle).unwrap();
        assert_eq!(state(&root).unwrap().watermark, 0);
        assert_eq!(
            load_selected(&root, &keys).unwrap().sequence,
            super::super::embedded().bundle.sequence
        );
        install_at(&root, &manifest, &signature, &package, &keys).unwrap();
        let old = Arc::new(RuleSnapshot { bundle });
        let (next_manifest, next_signature, mut next_package, _) = signed(11);
        next_package.push(b'x');
        assert!(install_at(&root, &next_manifest, &next_signature, &next_package, &keys).is_err());
        assert_eq!(state(&root).unwrap().current, 10);
        let (_, _, next_package, _) = signed(11);
        install_at(&root, &next_manifest, &next_signature, &next_package, &keys).unwrap();
        super::super::with_snapshot(old, || {
            assert_eq!(super::super::current().bundle.sequence, 10)
        });
        assert_eq!(load_selected(&root, &keys).unwrap().sequence, 11);
        assert_eq!(rollback_at(&root, &keys).unwrap().sequence, 10);
        assert_eq!(state(&root).unwrap().watermark, 11);
        assert!(install_at(&root, &manifest, &signature, &package, &keys).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cache_retains_current_previous_and_embedded_without_deleting_unknown_files() {
        let root = crate::core::testing::fixture("rule_retention");
        for sequence in 10..=12 {
            let (manifest, signature, package, keys) = signed(sequence);
            install_at(&root, &manifest, &signature, &package, &keys).unwrap();
        }
        assert!(!root.join("10").exists());
        let (_, _, _, keys) = signed(12);
        std::fs::create_dir(root.join("99")).unwrap();
        std::fs::write(root.join("99/keep"), b"unknown").unwrap();
        prune_packages(&root, &state(&root).unwrap(), &keys).unwrap();
        assert!(root.join("99/keep").is_file());
        std::fs::write(root.join("12/rules.json"), b"corrupt").unwrap();
        assert_eq!(load_selected(&root, &keys).unwrap().sequence, 11);
        assert_eq!(rollback_at(&root, &keys).unwrap().sequence, 11);
        assert_eq!(state(&root).unwrap().watermark, 12);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rules_only_release_reaches_the_next_scan_without_recompiling() {
        let root = crate::core::testing::fixture("rules_e2e_channel");
        std::fs::create_dir_all(&root).unwrap();
        // The publisher only edits a TOML-shaped rule inside the signed bundle.
        let mut bundle = super::super::embedded().bundle.clone();
        bundle.sequence = 7;
        let rule: super::super::RuleDefinition = toml::from_str(
            r#"
id = "e2e-fixture"
version = 1
platform = "all"
required = ["file"]

[[entries]]
root = "home"
path = "e2e-fixture-cache"
zh = "E2E 缓存"
en = "E2E cache"
category = "PackageCache"
operation = "contents"
disposal = "permanent"
recommended = true
"#,
        )
        .unwrap();
        bundle.rules.push(rule);
        bundle.validate().unwrap();
        let (manifest, signature, package, keys) = signed_bundle(&bundle);
        // A fake channel serves only the immutable release assets; the transport never recompiles.
        let package_url = format!(
            "https://github.com/meimingqi222/quick-cleaner/releases/download/rules-v{}/rules.json",
            bundle.sequence
        );
        let fetch = move |url: &str, limit: usize| -> Result<Vec<u8>, String> {
            let bytes = if url == format!("{CHANNEL}/manifest.json") {
                manifest.clone()
            } else if url == format!("{CHANNEL}/manifest.sig") {
                signature.clone()
            } else if url == package_url {
                package.clone()
            } else {
                return Err(format!("unexpected fetch {url}"));
            };
            if bytes.len() > limit {
                return Err("fixture transport limit".into());
            }
            Ok(bytes)
        };
        let installed = install_channel(CHANNEL, &root, &keys, &fetch)
            .unwrap()
            .expect("a newer channel installs");
        assert_eq!(installed.sequence, 7);
        // A second poll at the same sequence must not reinstall or lower the watermark.
        assert!(install_channel(CHANNEL, &root, &keys, &fetch)
            .unwrap()
            .is_none());
        assert_eq!(state(&root).unwrap().watermark, 7);
        // The already-compiled client loads the authenticated cache and the next scan surfaces
        // the added rule entry with its declared policy.
        let bundle = load_selected(&root, &keys).unwrap();
        let snapshot = Arc::new(RuleSnapshot { bundle });
        let mut targets = Vec::new();
        let home = root.join("home");
        super::super::with_snapshot(snapshot, || {
            super::super::append_path_targets(&mut targets, Some(&home));
        });
        let target = targets
            .iter()
            .find(|t| t.rule.id == "e2e-fixture")
            .expect("the published rule surfaces a scan target");
        assert_eq!(
            crate::core::safety::norm(&target.path),
            crate::core::safety::norm(&home.join("e2e-fixture-cache"))
        );
        assert_eq!(target.operation, crate::core::rules::Operation::Contents);
        assert!(target.recommended);
        // The signed channel cannot resurrect once the package is tampered after install:
        // `load_cached` treats an unauthenticated cache as absent and falls back to embedded.
        std::fs::write(root.join("7/rules.json"), b"corrupt").unwrap();
        let bundle = load_selected(&root, &keys)
            .ok()
            .map(|bundle| Arc::new(RuleSnapshot { bundle }))
            .unwrap_or_else(super::super::embedded);
        assert_eq!(
            bundle.bundle.sequence,
            super::super::embedded().bundle.sequence,
            "a corrupted accepted package falls back to embedded rules"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
