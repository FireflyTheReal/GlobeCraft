//! Keys for services that need one (roadmap adapter D): TomTom traffic, and later
//! others. A key comes from the environment
//! (`GLOBECRAFT_<SERVICE>_KEY`) or from Settings -> Keys, which saves it in
//! `service-keys.json` beside the caches (readable by the user only), never in
//! `settings.json`. Addresses name a key as `{key:<service>}`; it is filled in only
//! when a request is made, so a saved layer never holds the key itself.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

/// A service that needs a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyedService {
    /// In `{key:<id>}`, the environment variable and the keys file.
    pub id: &'static str,
    /// Display name and what the key is for: translation keys.
    pub name_key: &'static str,
    pub help_key: &'static str,
    /// Where a key is had (free for all of these).
    pub signup_url: &'static str,
    /// The only hosts an address may name for the key to be filled in: a key is sent
    /// to its own service and nowhere else.
    pub hosts: &'static [&'static str],
}

pub const TOMTOM: &str = "tomtom";

pub static SERVICES: &[KeyedService] = &[KeyedService {
    id: TOMTOM,
    name_key: "keys-tomtom",
    help_key: "keys-tomtom-help",
    signup_url: "https://developer.tomtom.com/",
    hosts: &["api.tomtom.com"],
}];

fn keys_file() -> PathBuf {
    crate::paths::cache_dir().join("service-keys.json")
}

/// The keys typed in Settings (the file's, then this session's edits).
static SAVED: LazyLock<RwLock<BTreeMap<String, String>>> =
    LazyLock::new(|| RwLock::new(read_private(&keys_file())));

fn environment_name(id: &str) -> String {
    format!("GLOBECRAFT_{}_KEY", id.to_ascii_uppercase())
}

/// The key for a service: from the environment, else the one saved. `None` when
/// neither has one.
pub fn get(id: &str) -> Option<String> {
    std::env::var(environment_name(id))
        .ok()
        .or_else(|| SAVED.read().ok()?.get(id).cloned())
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
}

/// Whether the environment sets the key (then Settings cannot change it).
pub fn from_environment(id: &str) -> bool {
    std::env::var(environment_name(id)).is_ok_and(|key| !key.trim().is_empty())
}

/// Sets a service's key for this session and returns every saved key, for
/// [`save`]. An empty key removes it.
pub fn set(id: &str, key: &str) -> BTreeMap<String, String> {
    let Ok(mut saved) = SAVED.write() else {
        return BTreeMap::new();
    };
    let key = key.trim();
    if key.is_empty() {
        saved.remove(id);
    } else {
        saved.insert(id.to_owned(), key.to_owned());
    }
    saved.clone()
}

/// Writes the keys file (only the user may read it); an empty map removes it. Runs
/// on the caller's thread: call it from a background task.
pub fn save(keys: &BTreeMap<String, String>) -> Result<(), crate::Error> {
    write_private(&keys_file(), keys)
}

/// `text` with every `{key:<service>}` replaced by that service's key; an error
/// naming the service when a key is missing, unknown, or the address is not on one
/// of the service's own hosts (a key is never sent anywhere else).
pub fn fill(text: &str) -> Result<String, crate::Error> {
    if !text.contains("{key:") {
        return Ok(text.to_owned());
    }
    let host = crate::network::host_of_url(text)
        .map(|authority| authority.split(':').next().unwrap_or(authority).to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{key:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "{key:".len()..];
        let end = after.find('}').ok_or("unclosed {key:…} in the address")?;
        let id = &after[..end];
        let service = SERVICES
            .iter()
            .find(|service| service.id == id)
            .ok_or_else(|| crate::Error::from(format!("no service named {id} takes a key")))?;
        if !host
            .as_deref()
            .is_some_and(|host| service.hosts.contains(&host))
        {
            return Err(format!(
                "a {id} key is only sent to {}; this address names another host",
                service.hosts.join(", ")
            )
            .into());
        }
        let key = get(id).ok_or_else(|| {
            crate::Error::from(format!(
                "this layer needs a {id} key: add it in Settings → Keys"
            ))
        })?;
        out.push_str(&key);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `text` with every key that [`fill`] could have put in it replaced by `{key:<id>}`,
/// for error messages and logs about a filled address.
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for service in SERVICES {
        if let Some(key) = get(service.id) {
            out = out.replace(&key, &format!("{{key:{}}}", service.id));
        }
    }
    out
}

/// An error from a request to a filled address, with any key taken out of its text
/// (HTTP errors name the address they failed for). Errors without a key keep their type.
pub(crate) fn redact_error(error: crate::Error) -> crate::Error {
    let text = error.to_string();
    let clean = redact(&text);
    if clean == text {
        error
    } else {
        crate::Error::from(clean)
    }
}

/// A JSON map of strings from a file, empty when it is missing or unreadable.
pub(crate) fn read_private(path: &Path) -> BTreeMap<String, String> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes a JSON map of strings readable by the user only; an empty map removes the
/// file.
pub(crate) fn write_private(
    path: &Path,
    keys: &BTreeMap<String, String>,
) -> Result<(), crate::Error> {
    if keys.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec(keys)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(path)?;
    file.write_all(&bytes)?;
    // An existing file keeps its old mode on open: tighten it too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fill_addresses_only_on_their_own_hosts_and_missing_ones_are_named() {
        set(TOMTOM, "  abc123 ");
        // Not filled in from the environment in this test.
        if !from_environment(TOMTOM) {
            assert_eq!(
                fill("https://api.tomtom.com/x/{key:tomtom}/y?a=1").unwrap(),
                "https://api.tomtom.com/x/abc123/y?a=1"
            );
            assert_eq!(
                redact("failed: https://api.tomtom.com/x/abc123/y"),
                "failed: https://api.tomtom.com/x/{key:tomtom}/y"
            );
        }
        assert_eq!(
            fill("https://x.example/plain").unwrap(),
            "https://x.example/plain"
        );
        // Another host never receives the key, however the address is spelled.
        for address in [
            "https://evil.example/{key:tomtom}",
            "https://api.tomtom.com.evil.example/{key:tomtom}",
            "https://api.tomtom.com@evil.example/{key:tomtom}",
            "file:///tmp/{key:tomtom}",
            "{key:tomtom}",
        ] {
            let error = fill(address).unwrap_err().to_string();
            assert!(error.contains("only sent to"), "{address}: {error}");
        }
        assert!(fill("https://x.example/{key:nobody}").is_err());
        assert!(fill("https://api.tomtom.com/{key:tomtom").is_err());
        let all = set(TOMTOM, "");
        assert!(!all.contains_key(TOMTOM));
        if !from_environment(TOMTOM) {
            let missing = fill("https://api.tomtom.com/{key:tomtom}")
                .unwrap_err()
                .to_string();
            assert!(missing.contains("Settings"), "{missing}");
        }
    }

    #[test]
    fn the_keys_file_is_private_and_removed_when_empty() {
        let path = std::env::temp_dir()
            .join(format!("globecraft-keys-{}", std::process::id()))
            .join("service-keys.json");
        let mut keys = BTreeMap::new();
        keys.insert("firms".to_owned(), "k".to_owned());
        write_private(&path, &keys).unwrap();
        assert_eq!(read_private(&path), keys);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        write_private(&path, &BTreeMap::new()).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
