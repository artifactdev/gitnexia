//! XDG locations (spec §14.3), through the `dirs` crate the app already uses for its caches:
//! relative XDG values are ignored, as the XDG spec requires. Without a home directory, the
//! temp dir stands in so callers always get a path. On Windows `dirs` gives the Known Folders:
//! config in `%APPDATA%` (roaming), cache and data in `%LOCALAPPDATA%` (this machine's: the
//! journal and temp index files name local paths).

use crate::platform::fs as pfs;
use std::path::{Path, PathBuf};

/// The app's folder name in each base folder: `GitNexia` on macOS (its `~/Library/Application
/// Support` and `~/Library/Caches` hold apps by their names), `gitnexia` elsewhere.
#[cfg(all(target_os = "macos", not(feature = "system-webview-beta")))]
pub const APP_DIR: &str = "GitNexia";
#[cfg(all(not(target_os = "macos"), not(feature = "system-webview-beta")))]
pub const APP_DIR: &str = "gitnexia";
#[cfg(all(target_os = "macos", feature = "system-webview-beta"))]
pub const APP_DIR: &str = "GitNexia Beta";
#[cfg(all(not(target_os = "macos"), feature = "system-webview-beta"))]
pub const APP_DIR: &str = "gitnexia-beta";

/// The folder names before the rename to GitNexia, newest first: `GitNex` (the app's name until
/// 1.6.0) and `GitVolt` (0.7.0 to 1.5.0). Their config and data are copied once on first start.
#[cfg(target_os = "macos")]
const LEGACY_APP_DIRS: &[&str] = &["GitNex", "GitVolt"];
#[cfg(not(target_os = "macos"))]
const LEGACY_APP_DIRS: &[&str] = &["gitnex", "gitvolt"];

/// The bundle identifiers whose CEF profile holds the UI's `localStorage` (theme mirror, drafts,
/// list prefs): GitNex (until 1.6.0) and GitVolt (0.7.0 to 1.5.0), newest first.
const LEGACY_CEF_DIRS: &[&str] = &["com.gitnex.desktop", "com.gitvolt.desktop"];
const CEF_DIR: &str = "com.gitnexia.desktop";

/// Copies an old config and data folder to GitNexia's names, once per process, before the store
/// opens. The old folders stay as they are (a downgrade still finds them); an existing new folder
/// is never overwritten. The CEF profile comes too, for the UI state localStorage keeps; the other
/// caches are left behind (they are re-fetched). Tokens in the data folder and the profiles in the
/// config folder move with it.
pub fn migrate_legacy_dirs() {
    if !legacy_migration_enabled() {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        copy_legacy_to(&config_base(), &config_dir());
        copy_legacy_to(&data_base(), &data_dir());
        copy_legacy_cef_profile();
    });
}

fn legacy_migration_enabled() -> bool {
    !cfg!(feature = "system-webview-beta")
}

/// Copies the first old app folder found under `base` to `new`, when `new` doesn't exist yet.
fn copy_legacy_to(base: &Path, new: &Path) {
    if new.exists() {
        return;
    }
    for old_name in LEGACY_APP_DIRS {
        let old = base.join(old_name);
        if old.is_dir() {
            if let Err(e) = copy_dir(&old, new) {
                tracing::warn!(target: "gitnexia_core::paths", "couldn't copy the old {} to {}: {e}", old.display(), new.display());
            }
            return;
        }
    }
}

/// The CEF profile, from the first old bundle identifier that has one (`{user cache}/{identifier}/
/// cef`, the runtime's default). A dev build's profile (`GITNEXIA_DEV_DIRS`) has a cache of its own.
fn copy_legacy_cef_profile() {
    let Some(base) = cache_base().filter(|_| dev_dirs().is_none()) else {
        return;
    };
    copy_legacy_cef_profile_at(&base);
}

fn copy_legacy_cef_profile_at(base: &Path) {
    let new = base.join(CEF_DIR).join("cef");
    if new.exists() {
        return;
    }
    for old_name in LEGACY_CEF_DIRS {
        let old = base.join(old_name).join("cef");
        if old.is_dir() {
            if let Err(e) = copy_dir(&old, &new) {
                tracing::warn!(target: "gitnexia_core::paths", "couldn't copy the old {} to {}: {e}", old.display(), new.display());
            }
            return;
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    #[cfg(unix)]
    pfs::set_mode(to, 0o700)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let meta = entry.file_type()?;
        if meta.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if meta.is_file() {
            std::fs::copy(entry.path(), &target)?;
            #[cfg(unix)]
            pfs::set_mode(&target, 0o600)?;
        }
        // Symlinks are left behind: settings and journals are regular files.
    }
    Ok(())
}

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir().filter(|p| p.is_absolute())
}

/// `~/.config/gitnexia` (`%APPDATA%\gitnexia`, `~/Library/Application Support/GitNexia`)
pub fn config_dir() -> PathBuf {
    config_base().join(APP_DIR)
}

/// The user's config folder (`~/.config`, `%APPDATA%`, `~/Library/Application Support`).
pub fn config_base() -> PathBuf {
    base_in(dev_dirs(), "config", dirs::config_dir).unwrap_or_else(std::env::temp_dir)
}

/// `~/.cache/gitnexia` (`%LOCALAPPDATA%\gitnexia`, `~/Library/Caches/GitNexia`)
pub fn cache_dir() -> PathBuf {
    cache_base()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DIR)
}

/// The user's cache folder (`~/.cache`, `%LOCALAPPDATA%`), if there's one.
pub fn cache_base() -> Option<PathBuf> {
    base_in(dev_dirs(), "cache", dirs::cache_dir)
}

/// `~/.local/share/gitnexia` (`%LOCALAPPDATA%\gitnexia`; spec #2 §5.1): the undo journal and temp
/// index files. Only `gitnexia-app` points the `Api` here; the harness and tests use a temp dir.
pub fn data_dir() -> PathBuf {
    data_base().join(APP_DIR)
}

/// The user's local data folder (`~/.local/share`, `%LOCALAPPDATA%`, `~/Library/Application Support`).
pub fn data_base() -> PathBuf {
    base_in(dev_dirs(), "data", dirs::data_local_dir).unwrap_or_else(std::env::temp_dir)
}

/// Debug builds only: `GITNEXIA_DEV_DIRS=<absolute dir>` puts the config, cache and data folders
/// in `<dir>\config`, `<dir>\cache` and `<dir>\data`, for a throwaway instance. Windows' Known
/// Folders can't be pointed elsewhere from the environment the way `XDG_*` can on Linux.
/// Release builds never read it.
#[cfg(debug_assertions)]
pub const DEV_DIRS: &str = "GITNEXIA_DEV_DIRS";

/// [`DEV_DIRS`]'s folder, when it's set to an absolute path (always none in a release build).
pub fn dev_dirs() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os(DEV_DIRS)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Some(dir);
    }
    None
}

fn base_in(
    dev_dirs: Option<PathBuf>,
    dev_name: &str,
    system: fn() -> Option<PathBuf>,
) -> Option<PathBuf> {
    dev_dirs.map(|dir| dir.join(dev_name)).or_else(system)
}

/// Where the askpass socket and the single-instance lock live: `$XDG_RUNTIME_DIR` (per-user,
/// 0700), else a private `gitnexia-<uid>` folder in the temp dir (see [`private_dir`]). Windows
/// has no runtime dir: `%TEMP%\gitnexia-<user>`, in the user's own temp dir.
pub fn runtime_dir() -> std::io::Result<PathBuf> {
    match dirs::runtime_dir() {
        Some(dir) => Ok(dir),
        None => private_dir(&std::env::temp_dir().join(format!("gitnexia-{}", pfs::user_tag()))),
    }
}

/// `path` as a folder only this user can use: created `0700` if it's missing; if it's there, it
/// must be a real folder (not a symlink someone planted in a shared temp dir) owned by this user,
/// and its mode is set back to `0700`. In a sticky temp dir nobody else can then swap it out.
/// (Windows: the folders GitNexia uses are in the user's profile, private by its ACL; see
/// [`crate::platform::fs`].)
pub(crate) fn private_dir(path: &Path) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    match pfs::private_dir_builder(&mut std::fs::DirBuilder::new()).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::other(format!(
            "{} isn't a directory",
            path.display()
        )));
    }
    if !pfs::owned_by_me(&meta) {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("{} belongs to another user", path.display()),
        ));
    }
    if pfs::mode(&meta) & 0o777 != 0o700 {
        pfs::set_mode(path, 0o700)?;
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `GITNEXIA_DEV_DIRS` replaces each system folder with its own subfolder; unset, the system
    /// folder is used.
    #[test]
    fn dev_dirs_stand_in_for_the_system_folders() {
        let dev = std::env::temp_dir().join("gb-dev");
        let system = || Some(PathBuf::from("/system/config"));
        assert_eq!(
            base_in(Some(dev.clone()), "config", system),
            Some(dev.join("config"))
        );
        assert_eq!(
            base_in(None, "config", system),
            Some(PathBuf::from("/system/config"))
        );
        assert_eq!(base_in(None, "cache", || None), None);
    }

    #[test]
    fn locations_are_absolute_and_namespaced() {
        for dir in [super::config_dir(), super::cache_dir(), super::data_dir()] {
            assert!(dir.is_absolute(), "{}", dir.display());
            assert!(dir.ends_with(super::APP_DIR), "{}", dir.display());
        }

        assert!(super::runtime_dir().unwrap().is_absolute());
    }

    #[test]
    fn beta_data_has_a_distinct_namespace() {
        if cfg!(feature = "system-webview-beta") {
            assert_ne!(APP_DIR, "GitNexia");
            assert_ne!(APP_DIR, "gitnexia");
            assert!(!legacy_migration_enabled());
        } else {
            assert!(APP_DIR == "GitNexia" || APP_DIR == "gitnexia");
            assert!(legacy_migration_enabled());
        }
    }

    #[test]
    fn the_newest_old_folder_is_copied_and_an_existing_one_is_never_overwritten() {
        let base = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let new = target.path().join("gitnexia");
        for (name, settings) in [
            (LEGACY_APP_DIRS[1], "oldest"),
            (LEGACY_APP_DIRS[0], "newest"),
        ] {
            std::fs::create_dir_all(base.path().join(name)).unwrap();
            std::fs::write(base.path().join(name).join("settings.json"), settings).unwrap();
        }
        copy_legacy_to(base.path(), &new);
        assert_eq!(
            std::fs::read_to_string(new.join("settings.json")).unwrap(),
            "newest"
        );
        std::fs::write(new.join("settings.json"), "keep").unwrap();
        copy_legacy_to(base.path(), &new);
        assert_eq!(
            std::fs::read_to_string(new.join("settings.json")).unwrap(),
            "keep"
        );
        assert_eq!(
            LEGACY_CEF_DIRS,
            &["com.gitnex.desktop", "com.gitvolt.desktop"]
        );
        assert_eq!(CEF_DIR, "com.gitnexia.desktop");
    }

    #[test]
    fn the_first_old_cef_profile_is_copied_and_an_existing_one_is_never_overwritten() {
        let base = tempfile::tempdir().unwrap();
        for (id, theme) in [
            ("com.gitvolt.desktop", "oldest"),
            ("com.gitnex.desktop", "newest"),
        ] {
            std::fs::create_dir_all(base.path().join(id).join("cef")).unwrap();
            std::fs::write(base.path().join(id).join("cef/Local Storage"), theme).unwrap();
        }
        copy_legacy_cef_profile_at(base.path());
        let new = base.path().join(CEF_DIR).join("cef");
        assert_eq!(
            std::fs::read_to_string(new.join("Local Storage")).unwrap(),
            "newest"
        );
        std::fs::write(new.join("Local Storage"), "keep").unwrap();
        copy_legacy_cef_profile_at(base.path());
        assert_eq!(
            std::fs::read_to_string(new.join("Local Storage")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn an_old_folder_is_copied_with_its_subfolders() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(old.path().join("profiles/default")).unwrap();
        std::fs::write(old.path().join("settings.json"), "{}").unwrap();
        std::fs::write(old.path().join("profiles/default/profile.json"), "{}").unwrap();
        let to = new.path().join("GitNexia");
        copy_dir(old.path(), &to).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("settings.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            std::fs::read_to_string(to.join("profiles/default/profile.json")).unwrap(),
            "{}"
        );
        assert!(!new.path().join("profiles").exists(), "no flattening");
    }

    /// The longest socket name bound in the runtime dir (the instance's) still fits a Unix
    /// socket address: 104 bytes with its NUL on macOS, whose runtime dir is in its long
    /// per-user `$TMPDIR` (`/var/folders/…/T/gitnexia-<uid>`).
    #[cfg(unix)]
    #[test]
    fn the_runtime_dir_leaves_room_for_the_sockets() {
        let longest = super::runtime_dir()
            .unwrap()
            .join(format!("gitnexia-instance-{:016x}.sock", u64::MAX));
        assert!(longest.as_os_str().len() < 104, "{}", longest.display());
    }

    #[test]
    fn the_private_fallback_refuses_a_file_and_reuses_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(format!("gitnexia-{}", pfs::user_tag()));
        assert_eq!(private_dir(&dir).unwrap(), dir);
        assert!(dir.is_dir());
        assert_eq!(
            private_dir(&dir).unwrap(),
            dir,
            "an existing one of ours is reused"
        );
        let file = tmp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(private_dir(&file).is_err());
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        pfs::mode(&std::fs::metadata(p).unwrap()) & 0o777
    }

    /// Unix only: Windows has no mode bits (the folder is private by its ACL).
    #[cfg(unix)]
    #[test]
    fn the_private_fallback_is_created_0700_and_its_mode_is_fixed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(format!("gitnexia-{}", pfs::user_tag()));
        assert_eq!(private_dir(&dir).unwrap(), dir);
        assert_eq!(mode(&dir), 0o700);
        pfs::set_mode(&dir, 0o755).unwrap();
        assert_eq!(
            private_dir(&dir).unwrap(),
            dir,
            "an existing one of ours is reused"
        );
        assert_eq!(mode(&dir), 0o700, "and made private again");
    }

    /// Unix only: no owner check on Windows, and `/` is root's only on Unix.
    #[cfg(unix)]
    #[test]
    fn the_private_fallback_refuses_a_symlink_a_file_or_someone_elses_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        pfs::set_mode(&target, 0o755).unwrap();
        let link = tmp.path().join("link");
        pfs::symlink(target.as_os_str(), &link).unwrap();
        assert!(private_dir(&link).is_err(), "a planted symlink");
        assert_eq!(mode(&target), 0o755, "its target is left alone");
        let file = tmp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(private_dir(&file).is_err());
        if !pfs::is_root() {
            assert_eq!(
                private_dir(Path::new("/")).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied,
                "owned by root, not us"
            );
        }
    }
}
