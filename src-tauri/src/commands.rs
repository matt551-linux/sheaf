use std::path::PathBuf;

use tauri::State;

use crate::engine::{
    Annotation, AnnotationPatch, AnnotationSpec, DocumentInfo, Engine, FormField, OutlineNode,
    PageText, RenderedPage, SaveOptions, SearchHit, StampSpec,
};
use crate::error::Result;

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
