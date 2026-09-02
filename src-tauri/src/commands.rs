use std::path::PathBuf;

use tauri::State;

use crate::engine::{DocumentInfo, Engine, OutlineNode, PageText, RenderedPage, SearchHit};
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
) -> Result<Vec<SearchHit>> {
    engine.search(id, query, case_sensitive.unwrap_or(false))
}
