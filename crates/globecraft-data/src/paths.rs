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

/// Deletes what the removed border pipeline (before 2026-10-25) downloaded: the whole
/// `geo` folder (national municipal files, geoBoundaries, Natural Earth detail levels,
/// Overpass answers and the old copy of the Wayback release list; often hundreds of MB)
/// and the empty `overture` folder. Nothing stores files there any more: do not reuse
/// these names. Returns the bytes freed. Cheap when there is nothing to remove (one
/// failed directory read); run it off the UI thread.
pub fn remove_retired_caches() -> u64 {
    remove_retired_in(&cache_dir())
}

fn remove_retired_in(root: &Path) -> u64 {
    let geo = root.join("geo");
    let freed = std::fs::read_dir(&geo)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.metadata().ok())
                .filter(std::fs::Metadata::is_file)
                .map(|meta| meta.len())
                .sum()
        })
        .unwrap_or(0);
    let removed = std::fs::remove_dir_all(&geo).is_ok();
    // Only removed when empty.
    let _ = std::fs::remove_dir(root.join("overture"));
    if removed { freed } else { 0 }
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

/// Writes `bytes` to `path` so that a crash, power loss or full disk leaves either the old
/// file or the new one, never a short one: a temporary file beside it is written, flushed
/// to disk and renamed over the target. `private` makes the file readable by the user only
/// (Unix).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut temp_name = path.file_name().unwrap_or_default().to_owned();
    temp_name.push(".tmp");
    let temp = path.with_file_name(temp_name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temp)?;
        // A temporary file left by an earlier attempt keeps its old mode on open.
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
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

    #[test]
    fn atomic_writes_replace_the_whole_file_and_leave_no_temporary_file() {
        let root = std::env::temp_dir().join(format!("globecraft-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("nested").join("settings.json");
        write_atomic(&path, b"{\"first\":1, \"long\":\"value\"}", false).unwrap();
        write_atomic(&path, b"{}", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        assert!(!root.join("nested").join("settings.json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // A destination that cannot be replaced fails and cleans up after itself.
        std::fs::create_dir_all(root.join("dir")).unwrap();
        assert!(write_atomic(&root.join("dir"), b"x", false).is_err());
        assert!(!root.join("dir.tmp").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn retired_downloads_go_and_other_data_stays() {
        let root = std::env::temp_dir().join(format!("globecraft-retired-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("geo")).unwrap();
        std::fs::create_dir_all(root.join("overture")).unwrap();
        std::fs::write(root.join("geo").join("wayback_config"), b"list").unwrap();
        std::fs::write(root.join("geo").join("old_municipal_file"), b"0123456789").unwrap();
        std::fs::write(root.join("settings.json"), b"{}").unwrap();
        assert_eq!(remove_retired_in(&root), 14);
        assert!(!root.join("geo").exists());
        assert!(!root.join("overture").exists());
        assert!(root.join("settings.json").exists());
        assert_eq!(remove_retired_in(&root), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
