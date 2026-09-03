mod commands;
pub mod edit;
pub mod engine;
mod error;
pub mod security;

use std::path::PathBuf;

use tauri::Manager;

/// Locate the PDFium shared library. Order:
/// 1. `SHEAF_PDFIUM_DIR` env var (developer override)
/// 2. next to the executable (how bundles ship it)
/// 3. `src-tauri/pdfium/<target>/` relative to the crate (dev runs)
/// 4. None, meaning fall back to a system-installed PDFium
pub fn pdfium_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SHEAF_PDFIUM_DIR") {
        return Some(PathBuf::from(dir));
    }
    let lib_name = pdfium_render::prelude::Pdfium::pdfium_platform_library_name();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Bundles ship the library as a Tauri resource. Where that lands:
            //   Windows (msi/nsis):  <exe dir>/resources/
            //   Linux (deb/rpm):     /usr/lib/<app>/resources/ (exe in /usr/bin)
            //   Linux (AppImage):    <exe dir>/../lib/<app>/resources/
            //   macOS:               <App>.app/Contents/Resources/resources/
            let candidates = [
                dir.to_path_buf(),
                dir.join("resources"),
                dir.join("..").join("Resources").join("resources"),
                dir.join("..").join("lib").join("sheaf").join("resources"),
                dir.join("..").join("lib").join("Sheaf").join("resources"),
                PathBuf::from("/usr/lib/sheaf/resources"),
                PathBuf::from("/usr/lib/Sheaf/resources"),
            ];
            for c in candidates {
                if c.join(&lib_name).exists() {
                    return Some(c);
                }
            }
        }
    }
    let target = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "win-arm64"
        } else {
            "win-x64"
        }
    } else if cfg!(target_os = "macos") {
        "mac-univ"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    };
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("pdfium")
        .join(target);
    if dev.join(&lib_name).exists() {
        return Some(dev);
    }
    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = engine::Engine::start(pdfium_dir())
        .unwrap_or_else(|e| panic!("Sheaf could not start its PDF engine: {e}"));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(engine)
        .setup(|app| {
            #[cfg(debug_assertions)]
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("Sheaf (dev)");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::launch_files,
            commands::open_document,
            commands::document_info,
            commands::close_document,
            commands::render_page,
            commands::page_text,
            commands::document_outline,
            commands::search_document,
            commands::save_attachment,
            commands::list_annotations,
            commands::add_annotation,
            commands::update_annotation,
            commands::delete_annotation,
            commands::undo,
            commands::redo,
            commands::save_document,
            commands::export_for_print,
            commands::list_form_fields,
            commands::set_form_field_value,
            commands::export_xfdf,
            commands::import_xfdf,
            commands::rotate_pages,
            commands::delete_pages,
            commands::move_pages,
            commands::insert_pages,
            commands::extract_pages,
            commands::crop_pages,
            commands::stamp_pages,
            commands::list_identities,
            commands::create_identity,
            commands::import_identity,
            commands::delete_identity,
            commands::sign_document,
            commands::list_signatures,
            commands::protect_document,
            commands::unprotect_document,
            commands::list_page_objects,
            commands::set_text_object,
            commands::move_page_object,
            commands::delete_page_object,
            commands::insert_image,
            commands::add_text,
            commands::extract_image,
            commands::list_links,
            commands::add_link,
            commands::create_from_images,
            commands::export_images,
            commands::export_text,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sheaf");
}
