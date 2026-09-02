use std::path::PathBuf;

use tauri::State;

use crate::engine::{
    Annotation, AnnotationPatch, AnnotationSpec, DocumentInfo, Engine, OutlineNode, PageText,
    RenderedPage, SaveOptions, SearchHit,
};
use crate::error::Result;

#[tauri::command]
pub fn open_document(
    engine: State<'_, Engine>,
    path: String,
    password: Option<String>,
) -> Result<DocumentInfo> {
    engine.open(PathBuf::from(path), password)
}

#[tauri::command]
pub fn document_info(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.info(id)
}

#[tauri::command]
pub fn close_document(engine: State<'_, Engine>, id: u32) -> Result<()> {
    engine.close(id)
}

#[tauri::command]
pub fn render_page(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    scale: f32,
    rotation: Option<u16>,
) -> Result<RenderedPage> {
    engine.render(id, page, scale, rotation.unwrap_or(0))
}

#[tauri::command]
pub fn page_text(engine: State<'_, Engine>, id: u32, page: u16) -> Result<PageText> {
    engine.text(id, page)
}

#[tauri::command]
pub fn document_outline(engine: State<'_, Engine>, id: u32) -> Result<Vec<OutlineNode>> {
    engine.outline(id)
}

#[tauri::command]
pub fn search_document(
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
pub fn save_attachment(engine: State<'_, Engine>, id: u32, index: u32, path: String) -> Result<()> {
    let bytes = engine.attachment(id, index)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[tauri::command]
pub fn list_annotations(engine: State<'_, Engine>, id: u32, page: u16) -> Result<Vec<Annotation>> {
    engine.list_annotations(id, page)
}

#[tauri::command]
pub fn add_annotation(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    spec: AnnotationSpec,
) -> Result<Annotation> {
    engine.add_annotation(id, page, spec)
}

#[tauri::command]
pub fn update_annotation(
    engine: State<'_, Engine>,
    id: u32,
    page: u16,
    index: u32,
    patch: AnnotationPatch,
) -> Result<Annotation> {
    engine.update_annotation(id, page, index, patch)
}

#[tauri::command]
pub fn delete_annotation(engine: State<'_, Engine>, id: u32, page: u16, index: u32) -> Result<()> {
    engine.delete_annotation(id, page, index)
}

#[tauri::command]
pub fn undo(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.undo(id)
}

#[tauri::command]
pub fn redo(engine: State<'_, Engine>, id: u32) -> Result<DocumentInfo> {
    engine.redo(id)
}

#[tauri::command]
pub fn save_document(
    engine: State<'_, Engine>,
    id: u32,
    options: SaveOptions,
) -> Result<DocumentInfo> {
    engine.save(id, options)
}

/// Write the current document state (including unsaved annotations) to a
/// temp file so the OS print pipeline can pick it up.
#[tauri::command]
pub fn export_for_print(engine: State<'_, Engine>, id: u32) -> Result<String> {
    let dir = std::env::temp_dir().join("sheaf-print");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("print-{id}-{}.pdf", std::process::id()));
    engine.save_copy(id, path.clone())?;
    Ok(path.to_string_lossy().into_owned())
}
