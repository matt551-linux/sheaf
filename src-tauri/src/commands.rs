use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::engine::{
    Annotation, AnnotationPatch, AnnotationSpec, DocumentInfo, Engine, FormField, OutlineNode,
    PageText, RenderedPage, SaveOptions, SearchHit, StampSpec,
};
use crate::edit::{ImageSpec, LinkInfo, LinkSpec, PageObject, TextSpec};
use crate::error::Result;
use crate::security::{Identity, IdentityStore, SecuritySpec, SignSpec, SignatureInfo};

/// PDF paths passed on the command line (file association / "Open with").
#[tauri::command]
pub async fn launch_files() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| a.to_lowercase().ends_with(".pdf") && std::path::Path::new(a).exists())
        .collect()
}

#[tauri::command]
pub async fn open_document(
    engine: State<'_, Engine>,
    path: String,
    password: Option<String>,
) -> Result<DocumentInfo> {
    engine.open(PathBuf::from(path), password)
}

#[tauri::command]
pub async fn document_info(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.info(id)
}

#[tauri::command]
pub async fn close_document(engine: State<'_, Engine>, id: u32) -> Result<()> {
    engine.close(id)
}

#[tauri::command]
pub async fn render_page(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    scale: f32,
    rotation: Option<u16>,
) -> Result<RenderedPage> {
    engine.render(id, page, scale, rotation.unwrap_or(0))
}

#[tauri::command]
pub async fn page_text(engine: State<'_, Engine>, id: u32, page: u16) -> Result<PageText> {
    engine.text(id, page)
}

#[tauri::command]
pub async fn document_outline(engine: State<'_, Engine>, id: u32) -> Result<Vec<OutlineNode>> {
    engine.outline(id)
}

#[tauri::command]
pub async fn search_document(
    engine: State<'_, Engine>,
    id: u32,
    query: String,
    case_sensitive: Option<bool>,
    whole_word: Option<bool>,
) -> Result<Vec<SearchHit>> {
    engine.search(
        id,
        query,
        case_sensitive.unwrap_or(false),
        whole_word.unwrap_or(false),
    )
}

#[tauri::command]
pub async fn save_attachment(
    engine: State<'_, Engine>,
    id: u32,
    index: u32,
    path: String,
) -> Result<()> {
    let bytes = engine.attachment(id, index)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[tauri::command]
pub async fn list_annotations(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
) -> Result<Vec<Annotation>> {
    engine.list_annotations(id, page)
}

#[tauri::command]
pub async fn add_annotation(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    spec: AnnotationSpec,
) -> Result<Annotation> {
    engine.add_annotation(id, page, spec)
}

#[tauri::command]
pub async fn update_annotation(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    index: u32,
    patch: AnnotationPatch,
) -> Result<Annotation> {
    engine.update_annotation(id, page, index, patch)
}

#[tauri::command]
pub async fn delete_annotation(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    index: u32,
) -> Result<()> {
    engine.delete_annotation(id, page, index)
}

#[tauri::command]
pub async fn undo(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.undo(id)
}

#[tauri::command]
pub async fn redo(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.redo(id)
}

#[tauri::command]
pub async fn save_document(
    engine: State<'_, Engine>,
    id: u32,
    options: SaveOptions,
) -> Result<DocumentInfo> {
    engine.save(id, options)
}

/// Write the current document state (including unsaved annotations) to a
/// temp file so callers can hand it to an external tool. (In-app printing
/// renders pages via the engine and prints from the main window.)
#[tauri::command]
pub async fn export_for_print(engine: State<'_, Engine>, id: u32) -> Result<String> {
    let dir = std::env::temp_dir().join("sheaf-print");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("print-{id}-{}.pdf", std::process::id()));
    engine.save_copy(id, path.clone())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn list_form_fields(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
) -> Result<Vec<FormField>> {
    engine.list_form_fields(id, page)
}

#[tauri::command]
pub async fn set_form_field_value(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    annot_index: u32,
    value: String,
) -> Result<FormField> {
    engine.set_form_field_value(id, page, annot_index, value)
}

#[tauri::command]
pub async fn export_xfdf(engine: State<'_, Engine>, id: u32, path: String) -> Result<()> {
    let xml = engine.export_xfdf(id)?;
    std::fs::write(path, xml)?;
    Ok(())
}

#[tauri::command]
pub async fn import_xfdf(engine: State<'_, Engine>, id: u32, path: String) -> Result<u32> {
    let xml = std::fs::read_to_string(path)?;
    engine.import_xfdf(id, xml)
}

#[tauri::command]
pub async fn rotate_pages(
    engine: State<'_, Engine>,
    id: u32,
    pages: Vec<u16>,
    delta: i32,
) -> Result<DocumentInfo> {
    engine.rotate_pages(id, pages, delta)
}

#[tauri::command]
pub async fn delete_pages(
    engine: State<'_, Engine>,
    id: u32,
    pages: Vec<u16>,
) -> Result<DocumentInfo> {
    engine.delete_pages(id, pages)
}

#[tauri::command]
pub async fn move_pages(
    engine: State<'_, Engine>,
    id: u32,
    pages: Vec<u16>,
    dest: u16,
) -> Result<DocumentInfo> {
    engine.move_pages(id, pages, dest)
}

#[tauri::command]
pub async fn insert_pages(
    engine: State<'_, Engine>,
    id: u32,
    path: String,
    password: Option<String>,
    at: u16,
) -> Result<DocumentInfo> {
    engine.insert_pages(id, PathBuf::from(path), password, at)
}

#[tauri::command]
pub async fn extract_pages(
    engine: State<'_, Engine>,
    id: u32,
    pages: Vec<u16>,
    path: String,
) -> Result<()> {
    engine.extract_pages(id, pages, PathBuf::from(path))
}

#[tauri::command]
pub async fn crop_pages(
    engine: State<'_, Engine>,
    id: u32,
    pages: Vec<u16>,
    crop_box: [f32; 4],
) -> Result<DocumentInfo> {
    engine.crop_pages(id, pages, crop_box)
}

#[tauri::command]
pub async fn stamp_pages(
    engine: State<'_, Engine>,
    id: u32,
    spec: StampSpec,
) -> Result<DocumentInfo> {
    engine.stamp_pages(id, spec)
}

// ---------- M5: sign and protect ----------

fn identities_dir(app: &AppHandle) -> Result<std::path::PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::SheafError::Engine(format!("app data dir: {e}")))?;
    Ok(base.join("identities"))
}

#[tauri::command]
pub async fn list_identities(app: AppHandle) -> Result<Vec<Identity>> {
    IdentityStore::new(identities_dir(&app)?).list()
}

#[tauri::command]
pub async fn create_identity(
    app: AppHandle,
    common_name: String,
    organization: Option<String>,
    password: String,
) -> Result<Identity> {
    IdentityStore::new(identities_dir(&app)?).create_self_signed(
        &common_name,
        organization.as_deref(),
        &password,
    )
}

#[tauri::command]
pub async fn import_identity(
    app: AppHandle,
    path: String,
    file_password: String,
    password: String,
) -> Result<Identity> {
    IdentityStore::new(identities_dir(&app)?).import_pkcs12(
        std::path::Path::new(&path),
        &file_password,
        &password,
    )
}

#[tauri::command]
pub async fn delete_identity(app: AppHandle, id: String) -> Result<()> {
    IdentityStore::new(identities_dir(&app)?).delete(&id)
}

#[tauri::command]
pub async fn sign_document(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: u32,
    spec: SignSpec,
) -> Result<DocumentInfo> {
    engine.sign_document(id, identities_dir(&app)?, spec)
}

#[tauri::command]
pub async fn list_signatures(engine: State<'_, Engine>, id: u32) -> Result<Vec<SignatureInfo>> {
    engine.list_signatures(id)
}

#[tauri::command]
pub async fn protect_document(
    engine: State<'_, Engine>,
    id: u32,
    spec: SecuritySpec,
) -> Result<DocumentInfo> {
    engine.protect(id, spec)
}

#[tauri::command]
pub async fn unprotect_document(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.unprotect(id)
}

// ---------- M6: edit ----------

#[tauri::command]
pub async fn list_page_objects(engine: State<'_, Engine>, id: u32, page: u16) -> Result<Vec<PageObject>> {
    engine.list_page_objects(id, page)
}

#[tauri::command]
pub async fn set_text_object(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    obj: u32,
    text: String,
    font_size: Option<f32>,
) -> Result<DocumentInfo> {
    engine.set_text_object(id, page, obj, text, font_size)
}

#[tauri::command]
pub async fn move_page_object(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    obj: u32,
    dx: f32,
    dy: f32,
    scale: Option<f32>,
) -> Result<DocumentInfo> {
    engine.move_page_object(id, page, obj, dx, dy, scale.unwrap_or(1.0))
}

#[tauri::command]
pub async fn delete_page_object(engine: State<'_, Engine>, id: u32, page: u16, obj: u32) -> Result<DocumentInfo> {
    engine.delete_page_object(id, page, obj)
}

#[tauri::command]
pub async fn insert_image(engine: State<'_, Engine>, id: u32, page: u16, spec: ImageSpec) -> Result<DocumentInfo> {
    engine.insert_image(id, page, spec)
}

#[tauri::command]
pub async fn extract_image(engine: State<'_, Engine>, id: u32, page: u16, obj: u32, path: String) -> Result<()> {
    engine.extract_image(id, page, obj, std::path::PathBuf::from(path))
}

#[tauri::command]
pub async fn list_links(engine: State<'_, Engine>, id: u32, page: u16) -> Result<Vec<LinkInfo>> {
    engine.list_links(id, page)
}

#[tauri::command]
pub async fn add_link(engine: State<'_, Engine>, id: u32, page: u16, spec: LinkSpec) -> Result<DocumentInfo> {
    engine.add_link(id, page, spec)
}

#[tauri::command]
pub async fn create_from_images(engine: State<'_, Engine>, paths: Vec<String>, out: String) -> Result<DocumentInfo> {
    engine.create_from_images(
        paths.into_iter().map(std::path::PathBuf::from).collect(),
        std::path::PathBuf::from(out),
    )
}

#[tauri::command]
pub async fn export_images(
    engine: State<'_, Engine>,
    id: u32,
    pages: Vec<u16>,
    dir: String,
    dpi: Option<f32>,
) -> Result<Vec<String>> {
    engine.export_images(id, pages, std::path::PathBuf::from(dir), dpi.unwrap_or(150.0))
}

#[tauri::command]
pub async fn export_text(engine: State<'_, Engine>, id: u32, pages: Vec<u16>, path: String) -> Result<()> {
    let text = engine.export_text(id, pages)?;
    std::fs::write(path, text)?;
    Ok(())
}

#[tauri::command]
pub async fn add_text(engine: State<'_, Engine>, id: u32, page: u16, spec: TextSpec) -> Result<DocumentInfo> {
    engine.add_text(id, page, spec)
}
