//! Per-user desktop integration for the portable Linux AppImage.
//!
//! AppImages deliberately do not install themselves. Sheaf offers an explicit,
//! reversible opt-in: copy the currently-running AppImage to a stable path,
//! create a launcher/MIME entry, and provide removal/uninstall controls.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Result, SheafError};

const DESKTOP_FILE: &str = "org.sheafpdf.sheaf.desktop";
const APPIMAGE_NAME: &str = "Sheaf.AppImage";

#[derive(Debug, Clone, Serialize)]
pub struct DesktopIntegrationStatus {
    pub supported: bool,
    pub is_appimage: bool,
    pub integrated: bool,
    pub appimage_path: Option<String>,
    pub is_default_pdf: bool,
}

#[derive(Debug, Clone)]
struct IntegrationPaths {
    appimage: PathBuf,
    desktop_file: PathBuf,
    icon: PathBuf,
}

impl IntegrationPaths {
    fn for_home(home: &Path) -> Self {
        Self {
            appimage: home.join(".local/opt/sheaf").join(APPIMAGE_NAME),
            desktop_file: home.join(".local/share/applications").join(DESKTOP_FILE),
            icon: home.join(".local/share/icons/hicolor/128x128/apps/sheaf.png"),
        }
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| SheafError::Engine("could not determine the home directory".into()))
}

fn appimage_source() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

fn quote_exec(path: &Path) -> String {
    // Desktop Entry Exec quoting: quote the fixed absolute program path. The
    // installation target contains no user-controlled filename components.
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

fn desktop_entry(appimage: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Sheaf\nComment=Open-source PDF reader and editor\nExec={} %F\nIcon=sheaf\nTerminal=false\nCategories=Office;Viewer;\nMimeType=application/pdf;\nStartupWMClass=sheaf\n",
        quote_exec(appimage)
    )
}

fn run_optional(command: &str, args: &[&str]) {
    let _ = std::process::Command::new(command).args(args).status();
}

fn refresh_databases(home: &Path) {
    let applications = home.join(".local/share/applications");
    let icons = home.join(".local/share/icons/hicolor");
    if let Some(path) = applications.to_str() {
        run_optional("update-desktop-database", &[path]);
    }
    if let Some(path) = icons.to_str() {
        run_optional("gtk-update-icon-cache", &["-f", path]);
    }
}

fn pdf_default_is_sheaf() -> bool {
    std::process::Command::new("xdg-mime")
        .args(["query", "default", "application/pdf"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == DESKTOP_FILE)
        .unwrap_or(false)
}

pub fn status() -> Result<DesktopIntegrationStatus> {
    #[cfg(not(target_os = "linux"))]
    {
        return Ok(DesktopIntegrationStatus {
            supported: false,
            is_appimage: false,
            integrated: false,
            appimage_path: None,
            is_default_pdf: false,
        });
    }

    #[cfg(target_os = "linux")]
    {
        let home = home_dir()?;
        let paths = IntegrationPaths::for_home(&home);
        let source = appimage_source();
        let integrated = paths.appimage.is_file() && paths.desktop_file.is_file();
        Ok(DesktopIntegrationStatus {
            supported: true,
            is_appimage: source.is_some(),
            integrated,
            appimage_path: paths
                .appimage
                .is_file()
                .then(|| paths.appimage.to_string_lossy().into_owned()),
            is_default_pdf: pdf_default_is_sheaf(),
        })
    }
}

#[cfg(target_os = "linux")]
fn copy_appimage(source: &Path, target: &Path) -> Result<()> {
    if source != target {
        let parent = target
            .parent()
            .ok_or_else(|| SheafError::Engine("invalid AppImage destination".into()))?;
        std::fs::create_dir_all(parent)?;
        let temporary = target.with_extension("AppImage.new");
        std::fs::copy(source, &temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(temporary, target)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_for(source: &Path, home: &Path) -> Result<DesktopIntegrationStatus> {
    let paths = IntegrationPaths::for_home(home);
    copy_appimage(source, &paths.appimage)?;
    std::fs::create_dir_all(paths.desktop_file.parent().unwrap())?;
    std::fs::create_dir_all(paths.icon.parent().unwrap())?;
    std::fs::write(&paths.desktop_file, desktop_entry(&paths.appimage))?;
    // Use the application icon compiled into the Tauri package rather than
    // extracting an AppImage, which avoids shelling out and works everywhere.
    std::fs::write(&paths.icon, include_bytes!("../icons/128x128.png"))?;
    refresh_databases(home);
    Ok(DesktopIntegrationStatus {
        supported: true,
        is_appimage: true,
        integrated: true,
        appimage_path: Some(paths.appimage.to_string_lossy().into_owned()),
        is_default_pdf: pdf_default_is_sheaf(),
    })
}

pub fn install() -> Result<DesktopIntegrationStatus> {
    #[cfg(not(target_os = "linux"))]
    {
        return Err(SheafError::Engine(
            "desktop integration is only available on Linux".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let source = appimage_source().ok_or_else(|| {
            SheafError::Engine(
                "desktop integration is available only when running from an AppImage".into(),
            )
        })?;
        install_for(&source, &home_dir()?)
    }
}

#[cfg(target_os = "linux")]
fn remove_path_if_exists(path: &Path, directory: bool) -> Result<()> {
    let result = if directory {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(target_os = "linux")]
fn remove_for(
    home: &Path,
    remove_appimage: bool,
    remove_data: bool,
    app_data_dir: Option<&Path>,
) -> Result<()> {
    let paths = IntegrationPaths::for_home(home);
    for path in [&paths.desktop_file, &paths.icon] {
        remove_path_if_exists(path, false)?;
    }
    if remove_appimage {
        remove_path_if_exists(&paths.appimage, false)?;
    }
    if remove_data {
        if let Some(dir) = app_data_dir {
            remove_path_if_exists(dir, true)?;
        }
    }
    refresh_databases(home);
    Ok(())
}

/// Remove only the launcher/icon registration. This intentionally does not
/// change the user's default PDF handler, because guessing a replacement can
/// silently break their desktop association.
pub fn remove_integration() -> Result<DesktopIntegrationStatus> {
    #[cfg(not(target_os = "linux"))]
    {
        return Err(SheafError::Engine(
            "desktop integration is only available on Linux".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let home = home_dir()?;
        remove_for(&home, false, false, None)?;
        status()
    }
}

/// Remove Sheaf's installed AppImage and desktop registration. Unix permits
/// unlinking the currently running file safely; the current process remains
/// alive until the frontend exits immediately afterwards.
pub fn uninstall(delete_data: bool, app_data_dir: Option<PathBuf>) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (delete_data, app_data_dir);
        return Err(SheafError::Engine(
            "AppImage uninstall is only available on Linux".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        remove_for(&home_dir()?, true, delete_data, app_data_dir.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_uses_a_stable_path_and_pdf_mime_type() {
        let appimage = Path::new("/home/alice/.local/opt/sheaf/Sheaf.AppImage");
        let entry = desktop_entry(appimage);
        assert!(entry.contains("Exec=\"/home/alice/.local/opt/sheaf/Sheaf.AppImage\" %F"));
        assert!(entry.contains("MimeType=application/pdf;"));
        assert!(entry.contains("StartupWMClass=sheaf"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn install_and_remove_only_touch_sheaf_owned_paths() {
        let root = std::env::temp_dir().join(format!("sheaf-integration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let source = root.join("Downloads/Sheaf.AppImage");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"appimage-test").unwrap();
        let data = root.join("app-data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("prefs.json"), b"{}").unwrap();

        let status = install_for(&source, &home).unwrap();
        let paths = IntegrationPaths::for_home(&home);
        assert!(status.integrated);
        assert!(paths.appimage.is_file());
        assert!(paths.desktop_file.is_file());
        assert!(paths.icon.is_file());
        assert_eq!(std::fs::read(&source).unwrap(), b"appimage-test");

        remove_for(&home, false, false, Some(&data)).unwrap();
        assert!(
            paths.appimage.is_file(),
            "remove integration keeps AppImage"
        );
        assert!(data.is_dir(), "remove integration keeps data");
        assert!(!paths.desktop_file.exists());
        assert!(!paths.icon.exists());

        remove_for(&home, true, true, Some(&data)).unwrap();
        assert!(!paths.appimage.exists());
        assert!(!data.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn integration_paths_are_scoped_to_the_user_home() {
        let paths = IntegrationPaths::for_home(Path::new("/tmp/sheaf-home"));
        assert_eq!(
            paths.appimage,
            Path::new("/tmp/sheaf-home/.local/opt/sheaf/Sheaf.AppImage")
        );
        assert_eq!(
            paths.desktop_file,
            Path::new("/tmp/sheaf-home/.local/share/applications/org.sheafpdf.sheaf.desktop")
        );
        assert_eq!(
            paths.icon,
            Path::new("/tmp/sheaf-home/.local/share/icons/hicolor/128x128/apps/sheaf.png")
        );
    }
}
