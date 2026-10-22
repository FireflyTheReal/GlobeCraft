use std::path::{Path, PathBuf};
/// Folder for settings, placemarks, keys and caches (the name predates the extra uses).
const DATA_FOLDER: &str = "globecraft_tiles";
/// The same folder before the project was renamed from EarthCraft.
const LEGACY_DATA_FOLDER: &str = "earthcraft_tiles";

fn data_base() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
}

pub(crate) fn cache_dir() -> PathBuf {
    data_base().join(DATA_FOLDER)
}

/// Copies settings, placemarks, keys and caches from an EarthCraft installation the first
/// time GlobeCraft starts. The old folder is left untouched. Returns whether anything
/// was copied. Call once at startup, before anything reads settings.
pub fn migrate_legacy_data() -> std::io::Result<bool> {
    migrate_folder(&data_base().join(LEGACY_DATA_FOLDER), &cache_dir())
}

fn migrate_folder(legacy: &Path, target: &Path) -> std::io::Result<bool> {
    if target.exists() || !legacy.is_dir() {
        return Ok(false);
    }
    // Copy next to the target and rename, so an interrupted copy is never mistaken
    // for a finished migration.
    let partial = target.with_extension("migrating");
    if partial.exists() {
        std::fs::remove_dir_all(&partial)?;
    }
    copy_tree(legacy, &partial)?;
    std::fs::rename(&partial, target)?;
    Ok(true)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

pub(crate) fn user_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}
/// Default output folder: `~/<parent>/GlobeCraft` (e.g. Pictures/GlobeCraft).
pub(crate) fn default_out_dir(parent: &str) -> String {
    let home = user_home();
    let base = if home.join(parent).is_dir() {
        home.join(parent)
    } else {
        home
    };
    base.join("GlobeCraft").to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_data_is_copied_once_and_left_in_place() {
        let root = std::env::temp_dir().join(format!("globecraft-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join(LEGACY_DATA_FOLDER);
        let target = root.join(DATA_FOLDER);
        std::fs::create_dir_all(legacy.join("geo")).unwrap();
        std::fs::write(legacy.join("settings.json"), b"{}").unwrap();
        std::fs::write(legacy.join("geo/a.json"), b"[]").unwrap();
        assert!(migrate_folder(&legacy, &target).unwrap());
        assert_eq!(std::fs::read(target.join("geo/a.json")).unwrap(), b"[]");
        assert!(legacy.join("settings.json").exists());
        // Later starts keep GlobeCraft's own data.
        std::fs::write(target.join("settings.json"), b"{\"new\":1}").unwrap();
        assert!(!migrate_folder(&legacy, &target).unwrap());
        assert_eq!(
            std::fs::read(target.join("settings.json")).unwrap(),
            b"{\"new\":1}"
        );
        // Nothing to migrate on a fresh machine.
        assert!(!migrate_folder(&root.join("missing"), &root.join("other")).unwrap());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
