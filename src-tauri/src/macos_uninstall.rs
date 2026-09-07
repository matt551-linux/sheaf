//! Uninstall controls for the DMG-installed macOS `.app` bundle.
//!
//! macOS has no package manager step to reverse: uninstalling means moving
//! `Sheaf.app` to the Trash. Sheaf offers an explicit, confirmed way to do
//! that from inside the app, plus a separate control that removes only
//! Sheaf's own local data (preferences, recents, OCR models, identities).
//!
//! The path detection and deletion planning below are pure functions over
//! paths so they can be tested on any platform without touching a real home
//! directory. Only the final "move to Trash" and "reveal in Finder" steps
//! are macOS-specific.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Result, SheafError};

/// Bundle identifier from `tauri.conf.json`; Tauri names the app-data
/// directory after it (`~/Library/Application Support/<identifier>`).
pub const BUNDLE_IDENTIFIER: &str = "org.sheafpdf.sheaf";

#[derive(Debug, Clone, Serialize)]
pub struct MacosInstallStatus {
    /// True only when running on macOS.
    pub supported: bool,
    /// Root of the running `.app` bundle, when the executable lives in one.
    pub app_bundle_path: Option<String>,
    /// True when the bundle sits inside a source checkout (`target/…`), in
    /// which case self-uninstall is refused.
    pub is_dev_checkout: bool,
    /// True when Sheaf was launched straight from the mounted disk image (or
    /// macOS App Translocation) rather than an installed copy.
    pub is_running_from_disk_image: bool,
    /// True when the bundle can be moved to the Trash from inside the app.
    pub can_move_to_trash: bool,
    /// Sheaf's local data directory, when it exists.
    pub app_data_path: Option<String>,
}

/// What an uninstall or data-removal action will touch. Built up front so the
/// destructive steps run against paths that have already been validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionPlan {
    /// Bundle to move to the Trash, if the bundle is being removed.
    pub trash_bundle: Option<PathBuf>,
    /// Local data directory to delete permanently, if requested.
    pub delete_app_data: Option<PathBuf>,
}

/// Derive the `.app` bundle root from the running executable's path.
///
/// A normal bundle lays out as `<Name>.app/Contents/MacOS/<executable>`. The
/// function returns the `<Name>.app` directory only when that exact structure
/// is present, so a bare binary (for example `cargo tauri dev`) yields `None`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn app_bundle_root(executable: &Path) -> Option<PathBuf> {
    let macos_dir = executable.parent()?;
    let contents_dir = macos_dir.parent()?;
    let bundle = contents_dir.parent()?;
    if macos_dir.file_name()? != "MacOS" || contents_dir.file_name()? != "Contents" {
        return None;
    }
    let name = bundle.file_name()?.to_str()?;
    if !name.ends_with(".app") || name == ".app" {
        return None;
    }
    Some(bundle.to_path_buf())
}

/// A bundle produced by a local build lives under the checkout's `target/`
/// directory (for example `src-tauri/target/release/bundle/macos/Sheaf.app`)
/// or beside a `Cargo.toml`. Never trash those: they are the developer's
/// build output, not an installation.
pub fn is_development_checkout(bundle: &Path) -> bool {
    bundle.ancestors().skip(1).any(|dir| {
        dir.file_name().is_some_and(|n| n == "target") || dir.join("Cargo.toml").is_file()
    })
}

/// An app opened directly from a mounted DMG runs under `/Volumes/<image>`,
/// and Gatekeeper may additionally run it from a read-only App Translocation
/// mount. Neither is an installation to uninstall; the user just ejects the
/// disk image.
pub fn is_running_from_disk_image(bundle: &Path) -> bool {
    bundle.starts_with("/Volumes")
        || bundle
            .components()
            .any(|c| c.as_os_str() == "AppTranslocation")
}

/// Guard rails on the bundle path before it is moved to the Trash.
pub fn validate_bundle(bundle: &Path) -> Result<()> {
    if !bundle.is_absolute() {
        return Err(SheafError::Engine("app bundle path is not absolute".into()));
    }
    if bundle.extension().and_then(|e| e.to_str()) != Some("app") {
        return Err(SheafError::Engine("path is not an .app bundle".into()));
    }
    if bundle.starts_with("/System") || bundle.starts_with("/usr") || bundle.starts_with("/Library")
    {
        return Err(SheafError::Engine(
            "refusing to remove a system-owned bundle".into(),
        ));
    }
    if is_development_checkout(bundle) {
        return Err(SheafError::Engine(
            "this Sheaf is running from a development checkout; delete the build output manually"
                .into(),
        ));
    }
    if is_running_from_disk_image(bundle) {
        return Err(SheafError::Engine(
            "this Sheaf is running from the disk image; quit Sheaf and eject the image instead"
                .into(),
        ));
    }
    if !bundle.join("Contents").join("MacOS").is_dir() {
        return Err(SheafError::Engine(
            "app bundle is missing Contents/MacOS".into(),
        ));
    }
    Ok(())
}

/// Guard rails on the app-data directory before it is deleted. Only a
/// directory named after Sheaf's bundle identifier is ever removed, so a
/// misconfigured path can never resolve to the user's home, Documents, or any
/// folder holding PDFs.
pub fn validate_app_data_dir(dir: &Path) -> Result<()> {
    if !dir.is_absolute() {
        return Err(SheafError::Engine("app data path is not absolute".into()));
    }
    let is_sheaf_dir = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == BUNDLE_IDENTIFIER);
    if !is_sheaf_dir {
        return Err(SheafError::Engine(format!(
            "refusing to delete a directory not named {BUNDLE_IDENTIFIER}"
        )));
    }
    if dir.components().count() < 3 {
        return Err(SheafError::Engine(
            "app data path is too shallow to be safe".into(),
        ));
    }
    Ok(())
}

/// Decide exactly what an action will remove. Validation failures are
/// surfaced here, before anything is touched.
pub fn plan(
    bundle: Option<&Path>,
    remove_bundle: bool,
    app_data_dir: Option<&Path>,
    delete_data: bool,
) -> Result<DeletionPlan> {
    let trash_bundle = if remove_bundle {
        let bundle = bundle
            .ok_or_else(|| SheafError::Engine("Sheaf is not running from an .app bundle".into()))?;
        validate_bundle(bundle)?;
        Some(bundle.to_path_buf())
    } else {
        None
    };
    let delete_app_data = if delete_data {
        let dir = app_data_dir.ok_or_else(|| {
            SheafError::Engine("could not determine the app data directory".into())
        })?;
        validate_app_data_dir(dir)?;
        // A missing directory means there is nothing to delete; not an error.
        dir.is_dir().then(|| dir.to_path_buf())
    } else {
        None
    };
    Ok(DeletionPlan {
        trash_bundle,
        delete_app_data,
    })
}

/// Delete Sheaf's local data directory. Only ever called with a path that
/// passed [`validate_app_data_dir`].
fn delete_app_data(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Carry out a plan using the supplied trash mechanism. `trash` is injected so
/// tests can exercise the ordering without a real Trash.
pub fn execute(plan: &DeletionPlan, trash: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    // Data first: once the bundle is in the Trash the user expects Sheaf to
    // be gone, and a data failure afterwards would be invisible.
    if let Some(dir) = &plan.delete_app_data {
        delete_app_data(dir)?;
    }
    if let Some(bundle) = &plan.trash_bundle {
        trash(bundle)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn current_bundle() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| app_bundle_root(&exe))
}

pub fn status(app_data_dir: Option<PathBuf>) -> MacosInstallStatus {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app_data_dir;
        MacosInstallStatus {
            supported: false,
            app_bundle_path: None,
            is_dev_checkout: false,
            is_running_from_disk_image: false,
            can_move_to_trash: false,
            app_data_path: None,
        }
    }
    #[cfg(target_os = "macos")]
    {
        let bundle = current_bundle();
        let is_dev_checkout = bundle.as_deref().is_some_and(is_development_checkout);
        let is_running_from_disk_image = bundle.as_deref().is_some_and(is_running_from_disk_image);
        let can_move_to_trash = bundle
            .as_deref()
            .is_some_and(|b| validate_bundle(b).is_ok());
        MacosInstallStatus {
            supported: true,
            app_bundle_path: bundle.map(|b| b.to_string_lossy().into_owned()),
            is_dev_checkout,
            is_running_from_disk_image,
            can_move_to_trash,
            app_data_path: app_data_dir
                .filter(|d| d.is_dir())
                .map(|d| d.to_string_lossy().into_owned()),
        }
    }
}

#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path) -> Result<()> {
    // NSFileManager.trashItemAtURL via the `trash` crate: native, keeps the
    // "Put Back" entry, and does not require Finder automation permission.
    trash::delete(path).map_err(|e| SheafError::Engine(format!("could not move to Trash: {e}")))
}

/// Move the running `Sheaf.app` to the Trash and optionally delete local
/// data. macOS allows moving a running bundle within the same volume; the
/// process stays alive until the frontend quits immediately afterwards.
pub fn uninstall(delete_data: bool, app_data_dir: Option<PathBuf>) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (delete_data, app_data_dir);
        Err(SheafError::Engine(
            "moving Sheaf to the Trash is only available on macOS".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    {
        let bundle = current_bundle();
        let plan = plan(
            bundle.as_deref(),
            true,
            app_data_dir.as_deref(),
            delete_data,
        )?;
        execute(&plan, move_to_trash)
    }
}

/// Delete only Sheaf's local data. Available on every platform since it never
/// touches the installation itself; the frontend quits afterwards so the
/// running app does not recreate preferences.
pub fn delete_local_data(app_data_dir: Option<PathBuf>) -> Result<()> {
    let plan = plan(None, false, app_data_dir.as_deref(), true)?;
    execute(&plan, |_| Ok(()))
}

/// Fallback when self-trashing is not possible: show the bundle in Finder so
/// the user can drag it to the Trash.
pub fn reveal_in_finder() -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        Err(SheafError::Engine(
            "Reveal in Finder is only available on macOS".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    {
        let bundle = current_bundle()
            .ok_or_else(|| SheafError::Engine("Sheaf is not running from an .app bundle".into()))?;
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&bundle)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(SheafError::Engine(
                "Finder could not reveal the app bundle".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct TempRoot(PathBuf);
    impl TempRoot {
        fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("sheaf-macos-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn make_bundle(dir: &Path) -> PathBuf {
        let bundle = dir.join("Sheaf.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/sheaf"), b"bin").unwrap();
        bundle
    }

    #[test]
    fn detects_bundle_root_from_executable_path() {
        let exe = Path::new("/Applications/Sheaf.app/Contents/MacOS/sheaf");
        assert_eq!(
            app_bundle_root(exe),
            Some(PathBuf::from("/Applications/Sheaf.app"))
        );
        let nested = Path::new("/Users/alice/Applications/My Sheaf.app/Contents/MacOS/sheaf");
        assert_eq!(
            app_bundle_root(nested),
            Some(PathBuf::from("/Users/alice/Applications/My Sheaf.app"))
        );
    }

    #[test]
    fn rejects_paths_that_are_not_bundle_layouts() {
        assert_eq!(
            app_bundle_root(Path::new(
                "/Users/alice/src/sheaf/src-tauri/target/debug/sheaf"
            )),
            None
        );
        assert_eq!(
            app_bundle_root(Path::new(
                "/Applications/Sheaf.app/Contents/Resources/sheaf"
            )),
            None
        );
        assert_eq!(
            app_bundle_root(Path::new("/Applications/Sheaf/Contents/MacOS/sheaf")),
            None
        );
        assert_eq!(
            app_bundle_root(Path::new("/.app/Contents/MacOS/sheaf")),
            None
        );
        assert_eq!(app_bundle_root(Path::new("sheaf")), None);
    }

    #[test]
    fn development_checkouts_are_recognised_by_target_dir_or_cargo_toml() {
        let root = TempRoot::new("dev");
        let built = make_bundle(&root.path().join("src-tauri/target/release/bundle/macos"));
        assert!(is_development_checkout(&built));

        let checkout = TempRoot::new("cargo");
        std::fs::write(checkout.path().join("Cargo.toml"), b"[package]").unwrap();
        let beside = make_bundle(&checkout.path().join("dist"));
        assert!(is_development_checkout(&beside));

        let installed = TempRoot::new("installed");
        let bundle = make_bundle(&installed.path().join("Applications"));
        assert!(!is_development_checkout(&bundle));
    }

    #[test]
    fn disk_image_and_translocated_launches_are_detected() {
        assert!(is_running_from_disk_image(Path::new(
            "/Volumes/Sheaf 0.2.3/Sheaf.app"
        )));
        assert!(is_running_from_disk_image(Path::new(
            "/private/var/folders/ab/T/AppTranslocation/9F2C/d/Sheaf.app"
        )));
        assert!(!is_running_from_disk_image(Path::new(
            "/Applications/Sheaf.app"
        )));
        assert!(!is_running_from_disk_image(Path::new(
            "/Users/alice/Applications/Sheaf.app"
        )));
    }

    #[test]
    fn validate_bundle_refuses_dev_system_and_malformed_paths() {
        let root = TempRoot::new("validate");
        let good = make_bundle(&root.path().join("Applications"));
        assert!(validate_bundle(&good).is_ok());

        let dev = make_bundle(&root.path().join("target/release/bundle/macos"));
        assert!(validate_bundle(&dev).is_err());

        let no_contents = root.path().join("Applications/Hollow.app");
        std::fs::create_dir_all(&no_contents).unwrap();
        assert!(validate_bundle(&no_contents).is_err());

        assert!(validate_bundle(Path::new("/System/Applications/Sheaf.app")).is_err());
        assert!(validate_bundle(Path::new("/Volumes/Sheaf 0.2.3/Sheaf.app")).is_err());
        assert!(validate_bundle(Path::new(
            "/private/var/folders/ab/T/AppTranslocation/9F2C/d/Sheaf.app"
        ))
        .is_err());
        assert!(validate_bundle(Path::new("/Applications/Sheaf")).is_err());
        assert!(validate_bundle(Path::new("relative/Sheaf.app")).is_err());
    }

    #[test]
    fn app_data_dir_must_be_named_after_the_bundle_identifier() {
        assert!(validate_app_data_dir(Path::new(
            "/Users/alice/Library/Application Support/org.sheafpdf.sheaf"
        ))
        .is_ok());
        assert!(validate_app_data_dir(Path::new("/Users/alice")).is_err());
        assert!(validate_app_data_dir(Path::new("/Users/alice/Documents")).is_err());
        assert!(
            validate_app_data_dir(Path::new("/Users/alice/Library/Application Support")).is_err()
        );
        assert!(
            validate_app_data_dir(Path::new("/org.sheafpdf.sheaf")).is_err(),
            "too shallow"
        );
        assert!(
            validate_app_data_dir(Path::new("Library/org.sheafpdf.sheaf")).is_err(),
            "relative"
        );
    }

    #[test]
    fn plan_keeps_data_by_default_and_only_includes_validated_paths() {
        let root = TempRoot::new("plan");
        let bundle = make_bundle(&root.path().join("Applications"));
        let data = root
            .path()
            .join("Library/Application Support")
            .join(BUNDLE_IDENTIFIER);
        std::fs::create_dir_all(&data).unwrap();

        let keep = plan(Some(&bundle), true, Some(&data), false).unwrap();
        assert_eq!(
            keep,
            DeletionPlan {
                trash_bundle: Some(bundle.clone()),
                delete_app_data: None
            }
        );

        let full = plan(Some(&bundle), true, Some(&data), true).unwrap();
        assert_eq!(
            full,
            DeletionPlan {
                trash_bundle: Some(bundle.clone()),
                delete_app_data: Some(data.clone())
            }
        );

        let data_only = plan(None, false, Some(&data), true).unwrap();
        assert_eq!(
            data_only,
            DeletionPlan {
                trash_bundle: None,
                delete_app_data: Some(data.clone())
            }
        );

        // Missing data directory: nothing to delete, still a valid plan.
        let missing = root.path().join("elsewhere").join(BUNDLE_IDENTIFIER);
        let none = plan(None, false, Some(&missing), true).unwrap();
        assert_eq!(none.delete_app_data, None);

        // No bundle when one is required, or an unsafe data dir, is an error.
        assert!(plan(None, true, Some(&data), false).is_err());
        assert!(plan(Some(&bundle), true, Some(root.path()), true).is_err());
        assert!(plan(Some(&bundle), true, None, true).is_err());
    }

    #[test]
    fn execute_deletes_only_sheaf_data_and_hands_bundle_to_trash() {
        let root = TempRoot::new("execute");
        let bundle = make_bundle(&root.path().join("Applications"));
        let support = root.path().join("Library/Application Support");
        let data = support.join(BUNDLE_IDENTIFIER);
        std::fs::create_dir_all(data.join("ocr-models")).unwrap();
        std::fs::write(data.join("prefs.json"), b"{}").unwrap();
        let other_app = support.join("com.example.other");
        std::fs::create_dir_all(&other_app).unwrap();
        let pdf = root.path().join("Documents/important.pdf");
        std::fs::create_dir_all(pdf.parent().unwrap()).unwrap();
        std::fs::write(&pdf, b"%PDF").unwrap();

        let trashed = Cell::new(None);
        let plan = plan(Some(&bundle), true, Some(&data), true).unwrap();
        execute(&plan, |p| {
            trashed.set(Some(p.to_path_buf()));
            Ok(())
        })
        .unwrap();

        assert_eq!(trashed.into_inner(), Some(bundle.clone()));
        assert!(bundle.is_dir(), "the fake trash did not remove the bundle");
        assert!(!data.exists(), "Sheaf data removed");
        assert!(other_app.is_dir(), "other apps' data untouched");
        assert!(pdf.is_file(), "user PDFs are never touched");
    }

    #[test]
    fn execute_stops_before_trashing_when_data_deletion_fails() {
        let root = TempRoot::new("order");
        let bundle = make_bundle(&root.path().join("Applications"));
        let data = root
            .path()
            .join("Library/Application Support")
            .join(BUNDLE_IDENTIFIER);
        let plan = DeletionPlan {
            trash_bundle: Some(bundle),
            delete_app_data: Some(data),
        };
        // Simulate the data step failing by making the trash step assert it
        // is never reached after a data error. remove_dir_all on a missing
        // directory is tolerated, so use a file in place of the directory.
        std::fs::create_dir_all(plan.delete_app_data.as_ref().unwrap().parent().unwrap()).unwrap();
        std::fs::write(plan.delete_app_data.as_ref().unwrap(), b"not a dir").unwrap();
        let reached_trash = Cell::new(false);
        let result = execute(&plan, |_| {
            reached_trash.set(true);
            Ok(())
        });
        assert!(result.is_err());
        assert!(!reached_trash.get());
    }

    #[test]
    fn status_reports_unsupported_off_macos_and_never_panics() {
        let s = status(None);
        assert_eq!(s.supported, cfg!(target_os = "macos"));
        if !s.supported {
            assert!(s.app_bundle_path.is_none());
            assert!(!s.can_move_to_trash);
        }
    }
}
