//! Windows installation awareness.
//!
//! The NSIS and MSI installers register a standard uninstaller with Windows,
//! so Sheaf never deletes its own executable or install directory. Instead it
//! reports whether the running copy is the registered install and, on
//! request, opens Windows "Installed apps" where the user runs the real
//! uninstaller. Removing Sheaf's own app data is a separate, explicitly
//! confirmed step handled by [`crate::local_data`].

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;

const PRODUCT_NAME: &str = "Sheaf";
/// Settings deep link to Apps > Installed apps.
const INSTALLED_APPS_URL: &str = "ms-settings:appsfeatures";

/// How the running executable relates to a Windows installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallContext {
    /// Running from a directory registered with a Windows uninstaller.
    Installed,
    /// Release build running from an unregistered location (unpacked or
    /// copied by hand). Removal is deleting the folder; there is nothing to
    /// uninstall.
    Portable,
    /// `tauri dev` or a debug build. Installation controls are unavailable.
    Dev,
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowsInstallStatus {
    /// False on every platform except Windows.
    pub supported: bool,
    pub context: InstallContext,
    /// Registered install directory, if Windows knows of one. Reported even
    /// when this copy is portable so the UI can point at the other install.
    pub install_location: Option<String>,
    pub installed_version: Option<String>,
    pub app_data_dir: Option<String>,
    pub app_data_present: bool,
}

/// One entry under `HKxx\Software\Microsoft\Windows\CurrentVersion\Uninstall`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallEntry {
    /// NSIS installs use the product name as the key; MSI uses a product GUID.
    pub key_name: String,
    pub display_name: Option<String>,
    pub install_location: Option<String>,
    pub display_version: Option<String>,
}

impl UninstallEntry {
    fn is_sheaf(&self) -> bool {
        self.key_name.eq_ignore_ascii_case(PRODUCT_NAME)
            || self
                .display_name
                .as_deref()
                .is_some_and(|n| n.trim().eq_ignore_ascii_case(PRODUCT_NAME))
    }

    fn location(&self) -> Option<PathBuf> {
        self.install_location
            .as_deref()
            .map(strip_quotes)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }
}

/// NSIS writes `InstallLocation` wrapped in double quotes.
pub fn strip_quotes(value: &str) -> &str {
    value.trim().trim_matches('"').trim()
}

/// Case-folded path components, with the `\\?\` verbatim prefix removed, so
/// two spellings of the same Windows directory compare equal.
fn normalized_components(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    Path::new(text)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

fn is_within(dir: &Path, ancestor: &Path) -> bool {
    let ancestor = normalized_components(ancestor);
    !ancestor.is_empty() && normalized_components(dir).starts_with(&ancestor)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub context: InstallContext,
    pub entry: Option<UninstallEntry>,
}

/// Decide the install context for `exe` from the registry `entries` and the
/// standard per-machine/per-user program roots (`Program Files`,
/// `%LOCALAPPDATA%\Programs`). Pure so it can be tested without a registry.
pub fn classify(
    exe: &Path,
    entries: &[UninstallEntry],
    program_roots: &[PathBuf],
    is_dev: bool,
) -> Classification {
    let sheaf: Vec<&UninstallEntry> = entries.iter().filter(|e| e.is_sheaf()).collect();
    let with_location = sheaf.iter().find(|e| e.location().is_some()).copied();
    if is_dev {
        return Classification {
            context: InstallContext::Dev,
            entry: with_location.or(sheaf.first().copied()).cloned(),
        };
    }
    let Some(exe_dir) = exe.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return Classification {
            context: InstallContext::Portable,
            entry: with_location.cloned(),
        };
    };
    // NSIS records the install directory; match the running copy against it.
    if let Some(entry) = sheaf
        .iter()
        .find(|e| e.location().is_some_and(|loc| is_within(exe_dir, &loc)))
    {
        return Classification {
            context: InstallContext::Installed,
            entry: Some((*entry).clone()),
        };
    }
    // MSI entries may omit InstallLocation; accept a Sheaf entry when the
    // executable sits under a standard program root.
    if let Some(entry) = sheaf.iter().find(|e| e.location().is_none()) {
        if program_roots.iter().any(|root| is_within(exe_dir, root)) {
            return Classification {
                context: InstallContext::Installed,
                entry: Some((*entry).clone()),
            };
        }
    }
    Classification {
        context: InstallContext::Portable,
        entry: with_location.cloned(),
    }
}

#[cfg(windows)]
fn registry_entries() -> Vec<UninstallEntry> {
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };
    use winreg::RegKey;

    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let views = [
        (HKEY_CURRENT_USER, KEY_READ),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
    ];
    let mut entries = Vec::new();
    for (hive, flags) in views {
        let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(UNINSTALL, flags) else {
            continue;
        };
        for key_name in root.enum_keys().flatten() {
            let Ok(key) = root.open_subkey_with_flags(&key_name, flags) else {
                continue;
            };
            let value = |name: &str| key.get_value::<String, _>(name).ok();
            let entry = UninstallEntry {
                display_name: value("DisplayName"),
                install_location: value("InstallLocation"),
                display_version: value("DisplayVersion"),
                key_name,
            };
            if entry.is_sheaf() {
                entries.push(entry);
            }
        }
    }
    entries
}

#[cfg(windows)]
fn program_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .filter_map(|var| std::env::var_os(var))
        .map(PathBuf::from)
        .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Programs"));
    }
    roots
}

pub fn status(app_data_dir: Option<PathBuf>) -> WindowsInstallStatus {
    let app_data_present = app_data_dir.as_deref().is_some_and(Path::is_dir);
    let app_data_dir = app_data_dir.map(|p| p.to_string_lossy().into_owned());

    #[cfg(not(windows))]
    {
        WindowsInstallStatus {
            supported: false,
            context: InstallContext::Portable,
            install_location: None,
            installed_version: None,
            app_data_dir,
            app_data_present,
        }
    }

    #[cfg(windows)]
    {
        let exe = std::env::current_exe().unwrap_or_default();
        let classified = classify(
            &exe,
            &registry_entries(),
            &program_roots(),
            crate::local_data::is_dev_build(),
        );
        let entry = classified.entry;
        WindowsInstallStatus {
            supported: true,
            context: classified.context,
            install_location: entry
                .as_ref()
                .and_then(UninstallEntry::location)
                .map(|p| p.to_string_lossy().into_owned()),
            installed_version: entry.and_then(|e| e.display_version),
            app_data_dir,
            app_data_present,
        }
    }
}

/// Open Windows Settings at Apps > Installed apps, where the registered
/// uninstaller lives. Falls back to the classic Programs and Features panel.
pub fn open_installed_apps() -> Result<()> {
    #[cfg(not(windows))]
    {
        Err(crate::error::SheafError::Engine(
            "Windows Installed apps is only available on Windows".into(),
        ))
    }

    #[cfg(windows)]
    {
        if tauri_plugin_opener::open_url(INSTALLED_APPS_URL, None::<&str>).is_ok() {
            return Ok(());
        }
        std::process::Command::new("control.exe")
            .arg("appwiz.cpl")
            .spawn()
            .map(drop)
            .map_err(|e| {
                crate::error::SheafError::Engine(format!(
                    "could not open Windows Installed apps: {e}"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(parts: &[&str]) -> PathBuf {
        let mut p = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        p.extend(parts);
        p
    }

    fn nsis_entry(location: &Path) -> UninstallEntry {
        UninstallEntry {
            key_name: "Sheaf".into(),
            display_name: Some("Sheaf".into()),
            // NSIS quotes the value, exactly as observed in the registry.
            install_location: Some(format!("\"{}\"", location.display())),
            display_version: Some("0.2.3".into()),
        }
    }

    fn msi_entry() -> UninstallEntry {
        UninstallEntry {
            key_name: "{6C1B2E4A-0000-4B7C-9D3A-5F2A1C0B8E11}".into(),
            display_name: Some("Sheaf".into()),
            install_location: None,
            display_version: Some("0.2.3".into()),
        }
    }

    fn other_entry() -> UninstallEntry {
        UninstallEntry {
            key_name: "Sheafmaker Pro".into(),
            display_name: Some("Sheafmaker Pro".into()),
            install_location: Some(abs(&["Program Files", "Sheafmaker"]).display().to_string()),
            display_version: Some("9.9".into()),
        }
    }

    #[test]
    fn strips_installer_quoting() {
        assert_eq!(strip_quotes("\"C:\\Users\\a\\AppData\\Local\\Sheaf\""), "C:\\Users\\a\\AppData\\Local\\Sheaf");
        assert_eq!(strip_quotes("  C:\\x  "), "C:\\x");
        assert_eq!(strip_quotes("\"\""), "");
    }

    #[test]
    fn path_containment_ignores_case_and_verbatim_prefix() {
        let install = abs(&["Users", "alice", "AppData", "Local", "Sheaf"]);
        let exe_dir = abs(&["users", "ALICE", "appdata", "local", "sheaf"]);
        assert!(is_within(&exe_dir, &install));
        assert!(is_within(&install.join("resources"), &install));
        assert!(!is_within(&abs(&["Users", "alice", "AppData", "Local", "Sheaf2"]), &install));
        assert!(!is_within(&abs(&["Users", "alice"]), &install));
        assert!(!is_within(&install, Path::new("")));
        if cfg!(windows) {
            let verbatim = PathBuf::from(format!(r"\\?\{}", install.display()));
            assert!(is_within(&verbatim, &install));
        }
    }

    #[test]
    fn running_from_the_registered_nsis_directory_is_installed() {
        let install = abs(&["Users", "alice", "AppData", "Local", "Sheaf"]);
        let exe = install.join("sheaf.exe");
        let entries = [other_entry(), nsis_entry(&install)];
        let c = classify(&exe, &entries, &[], false);
        assert_eq!(c.context, InstallContext::Installed);
        let entry = c.entry.unwrap();
        assert_eq!(entry.location().unwrap(), install);
        assert_eq!(entry.display_version.as_deref(), Some("0.2.3"));
    }

    #[test]
    fn a_copy_outside_the_registered_directory_is_portable_but_reports_the_install() {
        let install = abs(&["Users", "alice", "AppData", "Local", "Sheaf"]);
        let exe = abs(&["Users", "alice", "Downloads", "sheaf-unpacked", "sheaf.exe"]);
        let c = classify(&exe, &[nsis_entry(&install)], &[], false);
        assert_eq!(c.context, InstallContext::Portable);
        assert_eq!(c.entry.unwrap().location().unwrap(), install);
    }

    #[test]
    fn no_registry_entry_is_portable() {
        let exe = abs(&["Users", "alice", "Downloads", "sheaf.exe"]);
        let c = classify(&exe, &[other_entry()], &[abs(&["Program Files"])], false);
        assert_eq!(c.context, InstallContext::Portable);
        assert_eq!(c.entry, None);
    }

    #[test]
    fn msi_entry_without_location_matches_program_files_only() {
        let roots = [abs(&["Program Files"]), abs(&["Users", "alice", "AppData", "Local", "Programs"])];
        let installed = abs(&["Program Files", "Sheaf", "sheaf.exe"]);
        let c = classify(&installed, &[msi_entry()], &roots, false);
        assert_eq!(c.context, InstallContext::Installed);
        assert_eq!(c.entry.unwrap().key_name, msi_entry().key_name);

        let elsewhere = abs(&["Users", "alice", "Desktop", "sheaf.exe"]);
        let c = classify(&elsewhere, &[msi_entry()], &roots, false);
        assert_eq!(c.context, InstallContext::Portable);
        assert_eq!(c.entry, None, "no install location to report");
    }

    #[test]
    fn similarly_named_products_never_match() {
        let exe = abs(&["Program Files", "Sheafmaker", "sheaf.exe"]);
        let c = classify(&exe, &[other_entry()], &[abs(&["Program Files"])], false);
        assert_eq!(c.context, InstallContext::Portable);
    }

    #[test]
    fn dev_builds_are_never_installed_even_from_the_install_directory() {
        let install = abs(&["Users", "alice", "AppData", "Local", "Sheaf"]);
        let c = classify(&install.join("sheaf.exe"), &[nsis_entry(&install)], &[], true);
        assert_eq!(c.context, InstallContext::Dev);
        assert_eq!(c.entry.unwrap().location().unwrap(), install, "still reports the install");
    }

    /// Read-only scan of the real registry: must not panic and must only
    /// return Sheaf entries. Every entry that carries a location must parse
    /// to an absolute path once NSIS quoting is stripped.
    #[cfg(windows)]
    #[test]
    fn registry_scan_returns_only_sheaf_entries() {
        for entry in registry_entries() {
            assert!(entry.is_sheaf(), "unexpected entry {entry:?}");
            if let Some(location) = entry.location() {
                assert!(location.is_absolute(), "unquoted location {location:?}");
            }
        }
        assert!(program_roots().iter().all(|r| r.is_absolute()));
    }

    #[test]
    fn an_executable_without_a_directory_is_portable() {
        let c = classify(Path::new(""), &[], &[], false);
        assert_eq!(c.context, InstallContext::Portable);
    }
}
