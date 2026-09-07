//! Removal of Sheaf's own per-user app-data directory.
//!
//! This is the only place Sheaf deletes a directory tree it did not create in
//! the current session. It is deliberately narrow: the target must be an
//! absolute, real (non-linked) directory whose final component is the app
//! identifier, so a misconfigured path resolver can never point it at the
//! user's documents, home directory, or a filesystem root.

use std::path::{Component, Path};

use serde::Serialize;

use crate::error::{Result, SheafError};

/// `tauri dev` and debug builds share the production app-data directory, so
/// destructive install-related actions are refused there.
pub fn is_dev_build() -> bool {
    tauri::is_dev() || cfg!(debug_assertions)
}

/// Outcome of a local data removal. `removed` is false when there was nothing
/// on disk to remove, which the UI treats as success.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalDataRemoval {
    pub path: String,
    pub removed: bool,
}

fn refuse(reason: &str, dir: &Path) -> SheafError {
    SheafError::Engine(format!(
        "refusing to delete local data: {reason} ({})",
        dir.display()
    ))
}

/// Check that `dir` is a plausible Tauri app-data directory for `identifier`.
pub fn validate_app_data_dir(dir: &Path, identifier: &str) -> Result<()> {
    if identifier.is_empty() || identifier.contains(['/', '\\']) {
        return Err(refuse("invalid app identifier", dir));
    }
    if !dir.is_absolute() {
        return Err(refuse("path is not absolute", dir));
    }
    if dir
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(refuse("path contains relative components", dir));
    }
    if dir.file_name().and_then(|n| n.to_str()) != Some(identifier) {
        return Err(refuse("path is not the Sheaf app-data directory", dir));
    }
    // App data always lives at least two levels below a root
    // (`C:\Users\<user>\AppData\Roaming\<id>`, `~/.local/share/<id>`).
    // The grandparent must contain a real directory name, not just a drive
    // prefix or root separator.
    let deep_enough = dir
        .parent()
        .and_then(Path::parent)
        .map(|grandparent| {
            grandparent
                .components()
                .any(|c| matches!(c, Component::Normal(_)))
        })
        .unwrap_or(false);
    if !deep_enough {
        return Err(refuse("path is too close to a filesystem root", dir));
    }
    Ok(())
}

/// Delete the app-data directory after validating it. Symbolic links and
/// junctions are refused rather than followed or unlinked, because a linked
/// app-data directory means the user set something up by hand.
pub fn delete_app_data(dir: &Path, identifier: &str) -> Result<LocalDataRemoval> {
    validate_app_data_dir(dir, identifier)?;
    let path = dir.to_string_lossy().into_owned();
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(LocalDataRemoval { path, removed: false })
        }
        Err(e) => Err(e.into()),
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(refuse("path is a symbolic link or junction", dir))
        }
        Ok(meta) if !meta.is_dir() => Err(refuse("path is not a directory", dir)),
        Ok(_) => {
            std::fs::remove_dir_all(dir)?;
            Ok(LocalDataRemoval { path, removed: true })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const ID: &str = "org.sheafpdf.sheaf";

    /// A throwaway tree under the system temp directory. Nothing here ever
    /// touches a real user profile.
    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "sheaf-local-data-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home/AppData/Roaming")).unwrap();
            Self(root)
        }
        fn app_data(&self) -> PathBuf {
            self.0.join("home/AppData/Roaming").join(ID)
        }
        fn populate(&self) -> PathBuf {
            let data = self.app_data();
            std::fs::create_dir_all(data.join("identities")).unwrap();
            std::fs::create_dir_all(data.join("ocr-models")).unwrap();
            std::fs::write(data.join("sheaf-prefs.json"), b"{}").unwrap();
            std::fs::write(data.join("identities/alice.p12"), b"pkcs12").unwrap();
            std::fs::write(data.join("ocr-models/detection.rten"), b"model").unwrap();
            data
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn abs(parts: &[&str]) -> PathBuf {
        let mut p = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        p.extend(parts);
        p
    }

    #[test]
    fn accepts_the_real_app_data_layout() {
        validate_app_data_dir(&abs(&["Users", "alice", "AppData", "Roaming", ID]), ID).unwrap();
        validate_app_data_dir(&abs(&["home", "alice", ".local", "share", ID]), ID).unwrap();
    }

    #[test]
    fn rejects_relative_paths() {
        assert!(validate_app_data_dir(Path::new(ID), ID).is_err());
        assert!(validate_app_data_dir(&PathBuf::from("AppData/Roaming").join(ID), ID).is_err());
    }

    #[test]
    fn rejects_directories_that_are_not_the_app_identifier() {
        for last in ["Documents", "Roaming", "org.sheafpdf", "org.sheafpdf.sheaf.bak"] {
            let dir = abs(&["Users", "alice", "AppData", "Roaming", last]);
            assert!(
                validate_app_data_dir(&dir, ID).is_err(),
                "should refuse {}",
                dir.display()
            );
        }
        assert!(validate_app_data_dir(&abs(&["Users", "alice", "AppData", "Roaming"]), ID).is_err());
        // The identifier must be the final component, not merely present.
        assert!(validate_app_data_dir(&abs(&["Users", "alice", ID, "Documents"]), ID).is_err());
    }

    #[test]
    fn rejects_relative_components_and_bad_identifiers() {
        assert!(validate_app_data_dir(&abs(&["Users", "alice", "..", "..", ID]), ID).is_err());
        assert!(validate_app_data_dir(&abs(&["Users", "alice", "AppData", "..", ID]), ID).is_err());
        assert!(validate_app_data_dir(&abs(&["Users", "alice", "AppData", ID]), "").is_err());
        assert!(validate_app_data_dir(&abs(&["Users", "alice", "AppData", ID]), "a/b").is_err());
    }

    #[test]
    fn rejects_paths_near_a_filesystem_root() {
        assert!(validate_app_data_dir(&abs(&[ID]), ID).is_err());
        assert!(validate_app_data_dir(&abs(&["Users", ID]), ID).is_err());
        assert!(validate_app_data_dir(&abs(&["Users", "alice", ID]), ID).is_ok());
    }

    #[test]
    fn deletes_only_the_app_data_directory() {
        let sandbox = Sandbox::new("delete");
        let data = sandbox.populate();
        let roaming = data.parent().unwrap().to_path_buf();
        let sibling_pdf = sandbox.0.join("home/Documents/thesis.pdf");
        std::fs::create_dir_all(sibling_pdf.parent().unwrap()).unwrap();
        std::fs::write(&sibling_pdf, b"%PDF-1.7").unwrap();
        let other_app = roaming.join("org.example.other");
        std::fs::create_dir_all(&other_app).unwrap();
        std::fs::write(other_app.join("settings.json"), b"{}").unwrap();

        let result = delete_app_data(&data, ID).unwrap();
        assert!(result.removed);
        assert_eq!(result.path, data.to_string_lossy());
        assert!(!data.exists(), "app data removed");
        assert!(roaming.is_dir(), "parent directory kept");
        assert!(other_app.join("settings.json").is_file(), "other apps untouched");
        assert_eq!(std::fs::read(&sibling_pdf).unwrap(), b"%PDF-1.7", "user PDFs untouched");
    }

    #[test]
    fn missing_directory_is_not_an_error() {
        let sandbox = Sandbox::new("missing");
        let data = sandbox.app_data();
        let result = delete_app_data(&data, ID).unwrap();
        assert!(!result.removed);
        assert!(!data.exists());
    }

    #[test]
    fn refuses_a_file_at_the_app_data_path() {
        let sandbox = Sandbox::new("file");
        let data = sandbox.app_data();
        std::fs::write(&data, b"not a directory").unwrap();
        assert!(delete_app_data(&data, ID).is_err());
        assert!(data.is_file(), "file left alone");
    }

    #[test]
    fn refuses_when_validation_fails_without_touching_disk() {
        let sandbox = Sandbox::new("validate");
        let data = sandbox.populate();
        let wrong_id = "org.sheafpdf.other";
        assert!(delete_app_data(&data, wrong_id).is_err());
        assert!(data.join("sheaf-prefs.json").is_file());
        assert!(data.join("identities/alice.p12").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_app_data_directory() {
        let sandbox = Sandbox::new("symlink");
        let real = sandbox.0.join("real-data");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("sheaf-prefs.json"), b"{}").unwrap();
        let link = sandbox.app_data();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(delete_app_data(&link, ID).is_err());
        assert!(real.join("sheaf-prefs.json").is_file(), "link target untouched");
        assert!(std::fs::symlink_metadata(&link).is_ok(), "link itself kept");
    }
}
