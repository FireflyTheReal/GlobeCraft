//! The AI assistant's provider choice. API keys are not part of `settings.json`: they come from
//! the environment, the session, or (when the user opts in) `assistant-keys.json`, which only
//! the current user may read.
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AssistantPreferences {
    /// A provider preset id; empty until chosen.
    pub provider: String,
    /// Model per provider, so switching back keeps the last choice.
    pub models: BTreeMap<String, String>,
    /// Base address per provider, for local and compatible servers.
    pub base_urls: BTreeMap<String, String>,
    /// Whether keys typed in the settings are saved for the next launch.
    pub remember_keys: bool,
}

impl AssistantPreferences {
    pub(super) fn apply(&mut self, value: &serde_json::Value) {
        if let Some(provider) = value.get("provider").and_then(|v| v.as_str()) {
            self.provider = provider.to_string();
        }
        for (field, target) in [
            ("models", &mut self.models),
            ("base_urls", &mut self.base_urls),
        ] {
            if let Some(map) = value.get(field).and_then(|v| v.as_object()) {
                *target = map
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                    .collect();
            }
        }
        if let Some(remember) = value.get("remember_keys").and_then(|v| v.as_bool()) {
            self.remember_keys = remember;
        }
    }
}

fn keys_file() -> PathBuf {
    crate::paths::cache_dir().join("assistant-keys.json")
}

/// Saved API keys by provider id (empty when none were saved or the file is unreadable).
pub fn load_keys() -> BTreeMap<String, String> {
    std::fs::read(keys_file())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Replaces the saved API keys; an empty map removes the file. Runs on the caller's thread:
/// call it from a background task.
pub fn save_keys(keys: &BTreeMap<String, String>) -> Result<(), crate::Error> {
    let path = keys_file();
    if keys.is_empty() {
        return match std::fs::remove_file(&path) {
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
    let mut file = options.open(&path)?;
    file.write_all(&bytes)?;
    // An existing file keeps its old mode on open: tighten it too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_roundtrip_without_keys() {
        let mut prefs = AssistantPreferences {
            provider: "ollama".into(),
            remember_keys: true,
            ..Default::default()
        };
        prefs.models.insert("ollama".into(), "qwen3".into());
        let json = serde_json::to_value(&prefs).unwrap();
        assert!(
            !json.to_string().contains("key\":\""),
            "no key material in settings"
        );
        let mut restored = AssistantPreferences::default();
        restored.apply(&json);
        assert_eq!(restored, prefs);
        restored.apply(&serde_json::json!({"models": {"a": 1}, "remember_keys": "yes"}));
        assert!(restored.models.is_empty());
        assert!(restored.remember_keys);
    }
}
