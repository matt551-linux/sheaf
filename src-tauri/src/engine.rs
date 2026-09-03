//! The PDF engine runs on a single dedicated thread that owns the PDFium
//! bindings and every open document. Tauri commands talk to it through a
//! channel. This keeps all PDFium access serialized (PDFium is not thread
//! safe) and lets us hold raw handles without lifetime gymnastics.
//!
//! We use PDFium's C API directly (through the `pdfium-render` bindings loader)
//! because the high-level wrappers hide the document and page handles that
//! editing, saving, form rendering and undo need.
#![allow(non_upper_case_globals)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;

use base64::Engine as _;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{Result, SheafError};

pub type DocId = u32;

// PDFium public ABI constants (fpdfview.h, fpdf_annot.h, fpdf_save.h). The
// bindings crate exposes the types but not these values through its prelude.
const FPDF_ERR_PASSWORD: u32 = 4;
const FPDF_ANNOT: u32 = 1;
const FPDF_LCD_TEXT: u32 = 2;
const FPDFBitmap_BGRA: u32 = 4;
const FPDF_INCREMENTAL: u32 = 1;
const FPDF_NO_INCREMENTAL: u32 = 2;
const FLAT_NORMALDISPLAY: u32 = 0;
const FPDF_ANNOT_TEXT: u32 = 1;
const FPDF_ANNOT_LINK: u32 = 2;
const FPDF_ANNOT_FREETEXT: u32 = 3;
const FPDF_ANNOT_LINE: u32 = 4;
const FPDF_ANNOT_SQUARE: u32 = 5;
const FPDF_ANNOT_CIRCLE: u32 = 6;
const FPDF_ANNOT_POLYGON: u32 = 7;
const FPDF_ANNOT_POLYLINE: u32 = 8;
const FPDF_ANNOT_HIGHLIGHT: u32 = 9;
const FPDF_ANNOT_UNDERLINE: u32 = 10;
const FPDF_ANNOT_SQUIGGLY: u32 = 11;
const FPDF_ANNOT_STRIKEOUT: u32 = 12;
const FPDF_ANNOT_STAMP: u32 = 13;
const FPDF_ANNOT_INK: u32 = 15;
const FPDF_ANNOT_POPUP: u32 = 16;
const FPDF_ANNOT_FILEATTACHMENT: u32 = 17;
const FPDF_ANNOT_WIDGET: u32 = 20;
const FPDF_ANNOT_REDACT: u32 = 28;
const FPDF_ANNOT_FLAG_HIDDEN: u32 = 2;
const FPDF_ANNOT_FLAG_PRINT: u32 = 4;
const FPDF_ANNOT_FLAG_NOZOOM: u32 = 8;
const FPDF_ANNOT_FLAG_NOROTATE: u32 = 16;
const FPDF_ANNOT_APPEARANCEMODE_NORMAL: u32 = 0;
const FPDFANNOT_COLORTYPE_FPDFANNOT_COLORTYPE_Color: FPDFANNOT_COLORTYPE = 0;
const FPDFANNOT_COLORTYPE_FPDFANNOT_COLORTYPE_InteriorColor: FPDFANNOT_COLORTYPE = 1;

// Form field types (fpdf_formfill.h).
const FPDF_FORMFIELD_PUSHBUTTON: i32 = 1;
const FPDF_FORMFIELD_CHECKBOX: i32 = 2;
const FPDF_FORMFIELD_RADIOBUTTON: i32 = 3;
const FPDF_FORMFIELD_COMBOBOX: i32 = 4;
const FPDF_FORMFIELD_LISTBOX: i32 = 5;
const FPDF_FORMFIELD_TEXTFIELD: i32 = 6;
const FPDF_FORMFIELD_SIGNATURE: i32 = 7;

// Field flags (PDF 1.7 table 220/226/230).
const FIELD_FLAG_READONLY: u32 = 1;
const FIELD_FLAG_REQUIRED: u32 = 1 << 1;
const FIELD_FLAG_MULTILINE: u32 = 1 << 12;
const FIELD_FLAG_PASSWORD: u32 = 1 << 13;
const FIELD_FLAG_MULTISELECT: u32 = 1 << 21;

// ---------- Public data model (serialized to the frontend) ----------

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub index: u16,
    /// Width in PDF points (1/72 inch), after page rotation.
    pub width: f32,
    pub height: f32,
    pub rotation: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentInfo {
    pub index: u32,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentInfo {
    pub id: DocId,
    pub path: String,
    pub file_name: String,
    pub page_count: u16,
    pub pages: Vec<PageInfo>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub mod_date: Option<String>,
    pub file_size: u64,
    pub pdf_version: String,
    pub encrypted: bool,
    pub permissions: u32,
    pub attachments: Vec<AttachmentInfo>,
    pub modified: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineNode {
    pub title: String,
    pub page_index: Option<u16>,
    pub children: Vec<OutlineNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderedPage {
    pub index: u16,
    pub width_px: u32,
    pub height_px: u32,
    /// PNG encoded, base64 (data URL friendly).
    pub png_base64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextChar {
    pub ch: String,
    /// Loose bounds in PDF points, origin bottom-left (PDF user space).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageText {
    pub index: u16,
    pub text: String,
    pub chars: Vec<TextChar>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormFieldOption {
    pub label: String,
    pub selected: bool,
}

/// Text stamped onto pages: headers/footers, Bates numbering, or a diagonal
/// watermark. `text` supports `{n}` (page number, honoring `start_at`),
/// `{total}` (page count), and `{bates}` (zero-padded `start_at + i`).
#[derive(Debug, Clone, Deserialize)]
pub struct StampSpec {
    /// Pages to stamp; empty = all pages.
    pub pages: Vec<u16>,
    pub text: String,
    /// "header-left" | "header-center" | "header-right" |
    /// "footer-left" | "footer-center" | "footer-right" | "watermark"
    pub position: String,
    pub font_size: f32,
    pub color: Color,
    /// 0-255; watermarks typically use a low value.
    pub opacity: u8,
    /// First value for {n}/{bates} numbering.
    pub start_at: u32,
    /// Zero-pad width for {bates} (e.g. 6 -> 000001).
    pub bates_digits: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormField {
    pub page_index: u16,
    /// Annotation index of the widget on its page.
    pub annot_index: u32,
    /// Fully qualified field name ("T", dotted for hierarchies).
    pub name: String,
    /// Alternate/tooltip name ("TU"), shown to users.
    pub alt_name: String,
    /// "text" | "checkbox" | "radio" | "combo" | "listbox" | "button" | "signature" | "unknown"
    pub kind: String,
    pub value: String,
    pub rect: Rect,
    pub readonly: bool,
    pub required: bool,
    pub multiline: bool,
    pub password: bool,
    pub multiselect: bool,
    /// Export value for checkboxes/radios ("AS" on state), when available.
    pub export_value: String,
    pub checked: bool,
    pub options: Vec<FormFieldOption>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub page_index: u16,
    pub start: usize,
    pub len: usize,
    pub context: String,
    /// One rectangle per text line the match spans (PDF user space).
    pub rects: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AnnotKind {
    Text,
    FreeText,
    Line,
    Square,
    Circle,
    Polygon,
    PolyLine,
    Highlight,
    Underline,
    Squiggly,
    StrikeOut,
    Stamp,
    Ink,
    Link,
    Widget,
    Popup,
    FileAttachment,
    Redact,
    Other,
}

impl AnnotKind {
    fn from_subtype(s: FPDF_ANNOTATION_SUBTYPE) -> Self {
        match s as u32 {
            FPDF_ANNOT_TEXT => Self::Text,
            FPDF_ANNOT_FREETEXT => Self::FreeText,
            FPDF_ANNOT_LINE => Self::Line,
            FPDF_ANNOT_SQUARE => Self::Square,
            FPDF_ANNOT_CIRCLE => Self::Circle,
            FPDF_ANNOT_POLYGON => Self::Polygon,
            FPDF_ANNOT_POLYLINE => Self::PolyLine,
            FPDF_ANNOT_HIGHLIGHT => Self::Highlight,
            FPDF_ANNOT_UNDERLINE => Self::Underline,
            FPDF_ANNOT_SQUIGGLY => Self::Squiggly,
            FPDF_ANNOT_STRIKEOUT => Self::StrikeOut,
            FPDF_ANNOT_STAMP => Self::Stamp,
            FPDF_ANNOT_INK => Self::Ink,
            FPDF_ANNOT_LINK => Self::Link,
            FPDF_ANNOT_WIDGET => Self::Widget,
            FPDF_ANNOT_POPUP => Self::Popup,
            FPDF_ANNOT_FILEATTACHMENT => Self::FileAttachment,
            FPDF_ANNOT_REDACT => Self::Redact,
            _ => Self::Other,
        }
    }
    fn subtype(self) -> Option<FPDF_ANNOTATION_SUBTYPE> {
        Some(match self {
            Self::Text => FPDF_ANNOT_TEXT,
            Self::FreeText => FPDF_ANNOT_FREETEXT,
            Self::Square => FPDF_ANNOT_SQUARE,
            Self::Circle => FPDF_ANNOT_CIRCLE,
            Self::Highlight => FPDF_ANNOT_HIGHLIGHT,
            Self::Underline => FPDF_ANNOT_UNDERLINE,
            Self::Squiggly => FPDF_ANNOT_SQUIGGLY,
            Self::StrikeOut => FPDF_ANNOT_STRIKEOUT,
            Self::Ink => FPDF_ANNOT_INK,
            Self::Stamp => FPDF_ANNOT_STAMP,
            _ => return None,
        } as FPDF_ANNOTATION_SUBTYPE)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Full annotation state as read from the page.
#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub page_index: u16,
    pub index: u32,
    pub kind: AnnotKind,
    pub rect: Rect,
    pub contents: String,
    pub author: String,
    pub subject: String,
    pub modified: String,
    pub color: Option<Color>,
    pub interior_color: Option<Color>,
    pub border_width: f32,
    pub quads: Vec<[f32; 8]>,
    pub ink: Vec<Vec<[f32; 2]>>,
    pub hidden: bool,
    pub editable: bool,
}

/// What the frontend sends to create or update an annotation.
#[derive(Debug, Clone, Deserialize)]
pub struct AnnotationSpec {
    pub kind: AnnotKind,
    pub rect: Rect,
    #[serde(default)]
    pub contents: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub interior_color: Option<Color>,
    #[serde(default = "default_border")]
    pub border_width: f32,
    #[serde(default)]
    pub quads: Vec<[f32; 8]>,
    #[serde(default)]
    pub ink: Vec<Vec<[f32; 2]>>,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
}
fn default_border() -> f32 {
    1.0
}
fn default_font_size() -> f32 {
    12.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnnotationPatch {
    pub rect: Option<Rect>,
    pub contents: Option<String>,
    pub author: Option<String>,
    pub color: Option<Color>,
    pub interior_color: Option<Color>,
    pub border_width: Option<f32>,
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveOptions {
    pub path: Option<String>,
    #[serde(default)]
    pub flatten: bool,
}

// ---------- Engine handle (Send + Clone, lives in Tauri state) ----------

enum Request {
    Open(PathBuf, Option<String>, Sender<Result<DocumentInfo>>),
    Info(DocId, Sender<Result<DocumentInfo>>),
    Close(DocId, Sender<Result<()>>),
    Render(DocId, u16, f32, u16, Sender<Result<RenderedPage>>),
    Text(DocId, u16, Sender<Result<PageText>>),
    Outline(DocId, Sender<Result<Vec<OutlineNode>>>),
    Search(DocId, String, bool, bool, Sender<Result<Vec<SearchHit>>>),
    Attachment(DocId, u32, Sender<Result<Vec<u8>>>),
    ListAnnots(DocId, u16, Sender<Result<Vec<Annotation>>>),
    AddAnnot(DocId, u16, AnnotationSpec, Sender<Result<Annotation>>),
    UpdateAnnot(DocId, u16, u32, AnnotationPatch, Sender<Result<Annotation>>),
    DeleteAnnot(DocId, u16, u32, Sender<Result<()>>),
    Undo(DocId, Sender<Result<DocumentInfo>>),
    Redo(DocId, Sender<Result<DocumentInfo>>),
    Save(DocId, SaveOptions, Sender<Result<DocumentInfo>>),
    SaveCopy(DocId, PathBuf, Sender<Result<()>>),
    ListFormFields(DocId, u16, Sender<Result<Vec<FormField>>>),
    SetFormFieldValue(DocId, u16, u32, String, Sender<Result<FormField>>),
    ExportXfdf(DocId, Sender<Result<String>>),
    ImportXfdf(DocId, String, Sender<Result<u32>>),
    RotatePages(DocId, Vec<u16>, i32, Sender<Result<DocumentInfo>>),
    DeletePages(DocId, Vec<u16>, Sender<Result<DocumentInfo>>),
    MovePages(DocId, Vec<u16>, u16, Sender<Result<DocumentInfo>>),
    InsertPages(
        DocId,
        PathBuf,
        Option<String>,
        u16,
        Sender<Result<DocumentInfo>>,
    ),
    ExtractPages(DocId, Vec<u16>, PathBuf, Sender<Result<()>>),
    CropPages(DocId, Vec<u16>, [f32; 4], Sender<Result<DocumentInfo>>),
    StampPages(DocId, StampSpec, Sender<Result<DocumentInfo>>),
    // M5: security. These run on the engine thread because they replace the
    // live document bytes (and must go through undo/reload).
    SignDocument(DocId, PathBuf, crate::security::SignSpec, Sender<Result<DocumentInfo>>),
    ListSignatures(DocId, Sender<Result<Vec<crate::security::SignatureInfo>>>),
    Protect(DocId, crate::security::SecuritySpec, Sender<Result<DocumentInfo>>),
    Unprotect(DocId, Sender<Result<DocumentInfo>>),
}

#[derive(Clone)]
pub struct Engine {
    tx: Sender<Request>,
}

static ENGINE: std::sync::OnceLock<std::sync::Mutex<Option<Engine>>> = std::sync::OnceLock::new();

impl Engine {
    /// Get the process-wide engine, starting it on first use. PDFium can be
    /// bound only once per process, so the engine is a singleton; later calls
    /// ignore `library_dir` and return the existing handle.
    pub fn start(library_dir: Option<PathBuf>) -> Result<Self> {
        let slot = ENGINE.get_or_init(|| std::sync::Mutex::new(None));
        let mut guard = slot.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = guard.as_ref() {
            return Ok(e.clone());
        }
        let engine = Self::spawn(library_dir)?;
        *guard = Some(engine.clone());
        Ok(engine)
    }

    fn spawn(library_dir: Option<PathBuf>) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Request>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        thread::Builder::new()
            .name("sheaf-pdf-engine".into())
            .spawn(move || {
                let bindings = match bind_pdfium(library_dir.as_deref()) {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                unsafe { bindings.FPDF_InitLibrary() };
                let _ = ready_tx.send(Ok(()));
                let mut state = EngineState {
                    b: bindings,
                    docs: HashMap::new(),
                    next_id: 1,
                };
                while let Ok(req) = rx.recv() {
                    state.handle(req);
                }
            })
            .map_err(|e| SheafError::Engine(format!("failed to spawn engine thread: {e}")))?;
        ready_rx
            .recv()
            .map_err(|_| SheafError::Engine("engine thread exited during startup".into()))??;
        Ok(Self { tx })
    }

    fn call<T>(&self, build: impl FnOnce(Sender<Result<T>>) -> Request) -> Result<T> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(build(reply))
            .map_err(|_| SheafError::Engine("engine thread is gone".into()))?;
        rx.recv()
            .map_err(|_| SheafError::Engine("engine dropped the reply".into()))?
    }

    pub fn open(&self, path: PathBuf, password: Option<String>) -> Result<DocumentInfo> {
        self.call(|r| Request::Open(path, password, r))
    }
    pub fn info(&self, id: DocId) -> Result<DocumentInfo> {
        self.call(|r| Request::Info(id, r))
    }
    pub fn close(&self, id: DocId) -> Result<()> {
        self.call(|r| Request::Close(id, r))
    }
    pub fn render(&self, id: DocId, page: u16, scale: f32, rotation: u16) -> Result<RenderedPage> {
        self.call(|r| Request::Render(id, page, scale, rotation, r))
    }
    pub fn text(&self, id: DocId, page: u16) -> Result<PageText> {
        self.call(|r| Request::Text(id, page, r))
    }
    pub fn outline(&self, id: DocId) -> Result<Vec<OutlineNode>> {
        self.call(|r| Request::Outline(id, r))
    }
    pub fn search(&self, id: DocId, q: String, case: bool, whole: bool) -> Result<Vec<SearchHit>> {
        self.call(|r| Request::Search(id, q, case, whole, r))
    }
    pub fn attachment(&self, id: DocId, index: u32) -> Result<Vec<u8>> {
        self.call(|r| Request::Attachment(id, index, r))
    }
    pub fn list_annotations(&self, id: DocId, page: u16) -> Result<Vec<Annotation>> {
        self.call(|r| Request::ListAnnots(id, page, r))
    }
    pub fn add_annotation(&self, id: DocId, page: u16, spec: AnnotationSpec) -> Result<Annotation> {
        self.call(|r| Request::AddAnnot(id, page, spec, r))
    }
    pub fn update_annotation(
        &self,
        id: DocId,
        page: u16,
        index: u32,
        patch: AnnotationPatch,
    ) -> Result<Annotation> {
        self.call(|r| Request::UpdateAnnot(id, page, index, patch, r))
    }
    pub fn delete_annotation(&self, id: DocId, page: u16, index: u32) -> Result<()> {
        self.call(|r| Request::DeleteAnnot(id, page, index, r))
    }
    pub fn undo(&self, id: DocId) -> Result<DocumentInfo> {
        self.call(|r| Request::Undo(id, r))
    }
    pub fn redo(&self, id: DocId) -> Result<DocumentInfo> {
        self.call(|r| Request::Redo(id, r))
    }
    pub fn save(&self, id: DocId, opts: SaveOptions) -> Result<DocumentInfo> {
        self.call(|r| Request::Save(id, opts, r))
    }
    /// Write the current state to `path` without changing the open document.
    pub fn save_copy(&self, id: DocId, path: PathBuf) -> Result<()> {
        self.call(|r| Request::SaveCopy(id, path, r))
    }
    pub fn list_form_fields(&self, id: DocId, page: u16) -> Result<Vec<FormField>> {
        self.call(|r| Request::ListFormFields(id, page, r))
    }
    /// Set a field's value. For text/combo/listbox `value` is the text (listbox
    /// multiselect joins values with '\n'); for checkbox/radio it is "on"/"off".
    pub fn set_form_field_value(
        &self,
        id: DocId,
        page: u16,
        annot_index: u32,
        value: String,
    ) -> Result<FormField> {
        self.call(|r| Request::SetFormFieldValue(id, page, annot_index, value, r))
    }
    pub fn export_xfdf(&self, id: DocId) -> Result<String> {
        self.call(|r| Request::ExportXfdf(id, r))
    }
    /// Apply field values from an XFDF document; returns how many fields matched.
    pub fn import_xfdf(&self, id: DocId, xfdf: String) -> Result<u32> {
        self.call(|r| Request::ImportXfdf(id, xfdf, r))
    }
    /// Rotate pages by `delta` degrees (multiples of 90, may be negative).
    pub fn rotate_pages(&self, id: DocId, pages: Vec<u16>, delta: i32) -> Result<DocumentInfo> {
        self.call(|r| Request::RotatePages(id, pages, delta, r))
    }
    pub fn delete_pages(&self, id: DocId, pages: Vec<u16>) -> Result<DocumentInfo> {
        self.call(|r| Request::DeletePages(id, pages, r))
    }
    /// Move `pages` (in their current order) so the block starts at `dest`.
    pub fn move_pages(&self, id: DocId, pages: Vec<u16>, dest: u16) -> Result<DocumentInfo> {
        self.call(|r| Request::MovePages(id, pages, dest, r))
    }
    /// Insert every page of the PDF at `path` before index `at`.
    pub fn insert_pages(
        &self,
        id: DocId,
        path: PathBuf,
        password: Option<String>,
        at: u16,
    ) -> Result<DocumentInfo> {
        self.call(|r| Request::InsertPages(id, path, password, at, r))
    }
    /// Write `pages` (in the given order) to a new PDF at `path`.
    pub fn extract_pages(&self, id: DocId, pages: Vec<u16>, path: PathBuf) -> Result<()> {
        self.call(|r| Request::ExtractPages(id, pages, path, r))
    }
    /// Set the crop box (PDF points, [left, bottom, right, top]) on pages.
    pub fn crop_pages(&self, id: DocId, pages: Vec<u16>, box_: [f32; 4]) -> Result<DocumentInfo> {
        self.call(|r| Request::CropPages(id, pages, box_, r))
    }
    pub fn stamp_pages(&self, id: DocId, spec: StampSpec) -> Result<DocumentInfo> {
        self.call(|r| Request::StampPages(id, spec, r))
    }
    pub fn sign_document(
        &self,
        id: DocId,
        identities_dir: PathBuf,
        spec: crate::security::SignSpec,
    ) -> Result<DocumentInfo> {
        self.call(|r| Request::SignDocument(id, identities_dir, spec, r))
    }
    pub fn list_signatures(&self, id: DocId) -> Result<Vec<crate::security::SignatureInfo>> {
        self.call(|r| Request::ListSignatures(id, r))
    }
    pub fn protect(&self, id: DocId, spec: crate::security::SecuritySpec) -> Result<DocumentInfo> {
        self.call(|r| Request::Protect(id, spec, r))
    }
    pub fn unprotect(&self, id: DocId) -> Result<DocumentInfo> {
        self.call(|r| Request::Unprotect(id, r))
    }
}

fn bind_pdfium(library_dir: Option<&Path>) -> Result<Box<dyn PdfiumLibraryBindings>> {
    match library_dir {
        Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir))
            .or_else(|_| Pdfium::bind_to_system_library()),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|e| SheafError::Engine(format!("could not load PDFium: {e:?}")))
}

// ---------- Engine thread state ----------

const MAX_UNDO: usize = 40;

struct OpenDoc {
    /// The bytes PDFium is reading from. Must outlive `handle`.
    bytes: Vec<u8>,
    handle: FPDF_DOCUMENT,
    form: FPDF_FORMHANDLE,
    /// PDFium keeps a raw pointer to this for the life of `form`; it must not move or drop first.
    #[allow(dead_code)]
    form_info: Box<FPDF_FORMFILLINFO>,
    path: PathBuf,
    password: Option<String>,
    /// Serialized document state before each mutation (for undo).
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    modified: bool,
    /// True while `bytes` exactly equals PDFium's state (right after open,
    /// reload, sign, protect). PDFium-level edits clear it. When set, we
    /// serialize by copying `bytes`, which keeps signatures byte-identical.
    authoritative: bool,
}

struct EngineState {
    b: Box<dyn PdfiumLibraryBindings>,
    docs: HashMap<DocId, OpenDoc>,
    next_id: DocId,
}

/// RAII page handle with the form environment notified.
struct PageGuard<'a> {
    b: &'a dyn PdfiumLibraryBindings,
    form: FPDF_FORMHANDLE,
    page: FPDF_PAGE,
}
impl Drop for PageGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.form.is_null() {
                self.b.FORM_OnBeforeClosePage(self.page, self.form);
            }
            self.b.FPDF_ClosePage(self.page);
        }
    }
}

struct TextGuard<'a> {
    b: &'a dyn PdfiumLibraryBindings,
    tp: FPDF_TEXTPAGE,
}
impl Drop for TextGuard<'_> {
    fn drop(&mut self) {
        unsafe { self.b.FPDFText_ClosePage(self.tp) }
    }
}

impl EngineState {
    fn handle(&mut self, req: Request) {
        match req {
            Request::Open(path, pw, r) => {
                let _ = r.send(self.open(path, pw));
            }
            Request::Info(id, r) => {
                let _ = r.send(self.info(id));
            }
            Request::Close(id, r) => {
                if let Some(d) = self.docs.remove(&id) {
                    self.free(d);
                }
                let _ = r.send(Ok(()));
            }
            Request::Render(id, p, s, rot, r) => {
                let _ = r.send(self.render(id, p, s, rot));
            }
            Request::Text(id, p, r) => {
                let _ = r.send(self.page_text(id, p));
            }
            Request::Outline(id, r) => {
                let _ = r.send(self.outline(id));
            }
            Request::Search(id, q, c, w, r) => {
                let _ = r.send(self.search(id, &q, c, w));
            }
            Request::Attachment(id, i, r) => {
                let _ = r.send(self.attachment_bytes(id, i));
            }
            Request::ListAnnots(id, p, r) => {
                let _ = r.send(self.list_annotations(id, p));
            }
            Request::AddAnnot(id, p, spec, r) => {
                let _ = r.send(self.add_annotation(id, p, spec));
            }
            Request::UpdateAnnot(id, p, i, patch, r) => {
                let _ = r.send(self.update_annotation(id, p, i, patch));
            }
            Request::DeleteAnnot(id, p, i, r) => {
                let _ = r.send(self.delete_annotation(id, p, i));
            }
            Request::Undo(id, r) => {
                let _ = r.send(self.undo_redo(id, true));
            }
            Request::Redo(id, r) => {
                let _ = r.send(self.undo_redo(id, false));
            }
            Request::Save(id, o, r) => {
                let _ = r.send(self.save(id, o));
            }
            Request::SaveCopy(id, p, r) => {
                let _ = r.send((|| {
                    let d = self.doc(id)?;
                    let bytes = self.serialize(d)?;
                    std::fs::write(p, bytes)?;
                    Ok(())
                })());
            }
            Request::ListFormFields(id, p, r) => {
                let _ = r.send(self.list_form_fields(id, p));
            }
            Request::SetFormFieldValue(id, p, i, v, r) => {
                let _ = r.send(self.set_form_field_value(id, p, i, &v));
            }
            Request::ExportXfdf(id, r) => {
                let _ = r.send(self.export_xfdf(id));
            }
            Request::ImportXfdf(id, x, r) => {
                let _ = r.send(self.import_xfdf(id, &x));
            }
            Request::RotatePages(id, pages, delta, r) => {
                let _ = r.send(self.rotate_pages(id, &pages, delta));
            }
            Request::DeletePages(id, pages, r) => {
                let _ = r.send(self.delete_pages(id, &pages));
            }
            Request::MovePages(id, pages, dest, r) => {
                let _ = r.send(self.move_pages(id, &pages, dest));
            }
            Request::InsertPages(id, path, pw, at, r) => {
                let _ = r.send(self.insert_pages(id, &path, pw.as_deref(), at));
            }
            Request::ExtractPages(id, pages, path, r) => {
                let _ = r.send(self.extract_pages(id, &pages, &path));
            }
            Request::CropPages(id, pages, bx, r) => {
                let _ = r.send(self.crop_pages(id, &pages, bx));
            }
            Request::SignDocument(id, dir, spec, r) => {
                let _ = r.send(self.sign_document(id, dir, &spec));
            }
            Request::ListSignatures(id, r) => {
                let _ = r.send(self.list_signatures(id));
            }
            Request::Protect(id, spec, r) => {
                let _ = r.send(self.protect(id, &spec));
            }
            Request::Unprotect(id, r) => {
                let _ = r.send(self.unprotect(id));
            }
            Request::StampPages(id, spec, r) => {
                let _ = r.send(self.stamp_pages(id, &spec));
            }
        }
    }

    fn doc(&self, id: DocId) -> Result<&OpenDoc> {
        self.docs.get(&id).ok_or(SheafError::NoSuchDocument(id))
    }
    fn doc_mut(&mut self, id: DocId) -> Result<&mut OpenDoc> {
        self.docs.get_mut(&id).ok_or(SheafError::NoSuchDocument(id))
    }

    // ----- lifecycle -----

    /// Load a document from bytes and attach a form-fill environment so widgets render.
    fn load(
        &self,
        bytes: Vec<u8>,
        password: Option<&str>,
    ) -> Result<(
        Vec<u8>,
        FPDF_DOCUMENT,
        FPDF_FORMHANDLE,
        Box<FPDF_FORMFILLINFO>,
    )> {
        let handle = unsafe { self.b.FPDF_LoadMemDocument64(&bytes, password) };
        if handle.is_null() {
            let err = unsafe { self.b.FPDF_GetLastError() } as u32;
            return Err(if err == FPDF_ERR_PASSWORD {
                SheafError::PasswordRequired
            } else {
                SheafError::Pdf(format!("PDFium could not load the document (error {err})"))
            });
        }
        let mut info: Box<FPDF_FORMFILLINFO> = Box::new(unsafe { std::mem::zeroed() });
        info.version = 1;
        let form = unsafe { self.b.FPDFDOC_InitFormFillEnvironment(handle, &mut *info) };
        if !form.is_null() {
            unsafe {
                self.b.FPDF_SetFormFieldHighlightColor(form, 0, 0xE4F0FF as std::os::raw::c_ulong);
                self.b.FPDF_SetFormFieldHighlightAlpha(form, 100);
            }
        }
        Ok((bytes, handle, form, info))
    }

    fn free(&self, d: OpenDoc) {
        unsafe {
            if !d.form.is_null() {
                self.b.FPDFDOC_ExitFormFillEnvironment(d.form);
            }
            self.b.FPDF_CloseDocument(d.handle);
        }
        drop(d.bytes);
    }

    fn open(&mut self, path: PathBuf, password: Option<String>) -> Result<DocumentInfo> {
        let bytes = std::fs::read(&path)?;
        let (bytes, handle, form, form_info) = self.load(bytes, password.as_deref())?;
        let id = self.next_id;
        self.next_id += 1;
        self.docs.insert(
            id,
            OpenDoc {
                bytes,
                handle,
                form,
                form_info,
                path,
                password,
                undo: Vec::new(),
                redo: Vec::new(),
                modified: false,
                authoritative: true,
            },
        );
        self.info(id)
    }

    fn page(&self, d: &OpenDoc, index: u16) -> Result<PageGuard<'_>> {
        let page = unsafe { self.b.FPDF_LoadPage(d.handle, index as i32) };
        if page.is_null() {
            return Err(SheafError::NoSuchPage(index));
        }
        if !d.form.is_null() {
            unsafe { self.b.FORM_OnAfterLoadPage(page, d.form) };
        }
        Ok(PageGuard {
            b: self.b.as_ref(),
            form: d.form,
            page,
        })
    }

    fn meta(&self, d: &OpenDoc, tag: &str) -> Option<String> {
        let b = &self.b;
        let len = unsafe { b.FPDF_GetMetaText(d.handle, tag, std::ptr::null_mut(), 0) };
        if len <= 2 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize) / 2];
        unsafe { b.FPDF_GetMetaText(d.handle, tag, buf.as_mut_ptr() as *mut c_void, len) };
        let s = utf16_to_string(&buf);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    fn info(&self, id: DocId) -> Result<DocumentInfo> {
        let d = self.doc(id)?;
        let b = &self.b;
        let count = unsafe { b.FPDF_GetPageCount(d.handle) }.max(0) as u16;
        let mut pages = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut size = FS_SIZEF {
                width: 0.0,
                height: 0.0,
            };
            unsafe { b.FPDF_GetPageSizeByIndexF(d.handle, i as i32, &mut size) };
            // Rotation needs a loaded page; cheap enough for typical documents.
            let rotation = self
                .page(d, i)
                .map(|p| unsafe { b.FPDFPage_GetRotation(p.page) } * 90)
                .unwrap_or(0) as u16;
            pages.push(PageInfo {
                index: i,
                width: size.width,
                height: size.height,
                rotation,
            });
        }
        let attachments = {
            let n = unsafe { b.FPDFDoc_GetAttachmentCount(d.handle) }.max(0);
            (0..n)
                .filter_map(|i| {
                    let a = unsafe { b.FPDFDoc_GetAttachment(d.handle, i) };
                    if a.is_null() {
                        return None;
                    }
                    let len = unsafe { b.FPDFAttachment_GetName(a, std::ptr::null_mut(), 0) };
                    let mut buf = vec![0u16; (len as usize) / 2];
                    unsafe { b.FPDFAttachment_GetName(a, buf.as_mut_ptr(), len) };
                    let mut size: std::os::raw::c_ulong = 0;
                    unsafe { b.FPDFAttachment_GetFile(a, std::ptr::null_mut(), 0, &mut size) };
                    Some(AttachmentInfo {
                        index: i as u32,
                        name: utf16_to_string(&buf),
                        size: size as u64,
                    })
                })
                .collect()
        };
        let version = {
            let mut v: i32 = 0;
            unsafe { b.FPDF_GetFileVersion(d.handle, &mut v) };
            format!("{}.{}", v / 10, v % 10)
        };
        let encrypted = unsafe { b.FPDF_GetSecurityHandlerRevision(d.handle) } != -1;
        Ok(DocumentInfo {
            id,
            path: d.path.to_string_lossy().into_owned(),
            file_name: d
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            page_count: count,
            pages,
            title: self.meta(d, "Title"),
            author: self.meta(d, "Author"),
            subject: self.meta(d, "Subject"),
            keywords: self.meta(d, "Keywords"),
            creator: self.meta(d, "Creator"),
            producer: self.meta(d, "Producer"),
            creation_date: self.meta(d, "CreationDate"),
            mod_date: self.meta(d, "ModDate"),
            file_size: d.bytes.len() as u64,
            pdf_version: version,
            encrypted,
            permissions: unsafe { b.FPDF_GetDocPermissions(d.handle) } as u32,
            attachments,
            modified: d.modified,
            can_undo: !d.undo.is_empty(),
            can_redo: !d.redo.is_empty(),
        })
    }

    // ----- rendering -----

    fn render(&self, id: DocId, index: u16, scale: f32, rotation: u16) -> Result<RenderedPage> {
        let d = self.doc(id)?;
        let b = &self.b;
        let p = self.page(d, index)?;
        let scale = scale.clamp(0.05, 16.0);
        let pw = unsafe { b.FPDF_GetPageWidthF(p.page) };
        let ph = unsafe { b.FPDF_GetPageHeightF(p.page) };
        let rot = ((rotation % 360) / 90) as i32;
        let (w, h) = if rot % 2 == 1 { (ph, pw) } else { (pw, ph) };
        let w = ((w * scale).round() as i32).max(1);
        let h = ((h * scale).round() as i32).max(1);

        let bmp =
            unsafe { b.FPDFBitmap_CreateEx(w, h, FPDFBitmap_BGRA as i32, std::ptr::null_mut(), 0) };
        if bmp.is_null() {
            return Err(SheafError::Pdf("could not allocate bitmap".into()));
        }
        let flags = (FPDF_ANNOT | FPDF_LCD_TEXT) as i32;
        unsafe {
            b.FPDFBitmap_FillRect(bmp, 0, 0, w, h, 0xFFFFFFFF as std::os::raw::c_ulong);
            b.FPDF_RenderPageBitmap(bmp, p.page, 0, 0, w, h, rot, flags);
            if !d.form.is_null() {
                b.FPDF_FFLDraw(d.form, bmp, p.page, 0, 0, w, h, rot, flags);
            }
        }
        let stride = unsafe { b.FPDFBitmap_GetStride(bmp) } as usize;
        let buf = unsafe { b.FPDFBitmap_GetBuffer(bmp) } as *const u8;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            let row = unsafe { std::slice::from_raw_parts(buf.add(y * stride), (w * 4) as usize) };
            let out = &mut rgba[y * (w as usize) * 4..(y + 1) * (w as usize) * 4];
            for (src, dst) in row.as_chunks::<4>().0.iter().zip(out.chunks_exact_mut(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = 255;
            }
        }
        unsafe { b.FPDFBitmap_Destroy(bmp) };

        let mut png = Vec::new();
        image::write_buffer_with_format(
            &mut std::io::Cursor::new(&mut png),
            &rgba,
            w as u32,
            h as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|e| SheafError::Pdf(format!("png encode failed: {e}")))?;
        Ok(RenderedPage {
            index,
            width_px: w as u32,
            height_px: h as u32,
            png_base64: base64::engine::general_purpose::STANDARD.encode(png),
        })
    }

    // ----- text -----

    fn text_page<'a>(&'a self, p: &PageGuard<'_>) -> Result<TextGuard<'a>> {
        let tp = unsafe { self.b.FPDFText_LoadPage(p.page) };
        if tp.is_null() {
            return Err(SheafError::Pdf("text layer unavailable".into()));
        }
        Ok(TextGuard {
            b: self.b.as_ref(),
            tp,
        })
    }

    fn chars_of(&self, tp: FPDF_TEXTPAGE) -> (String, Vec<TextChar>) {
        let b = &self.b;
        let n = unsafe { b.FPDFText_CountChars(tp) }.max(0);
        let mut text = String::with_capacity(n as usize);
        let mut chars = Vec::with_capacity(n as usize);
        for i in 0..n {
            let cp = unsafe { b.FPDFText_GetUnicode(tp, i) };
            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
            text.push(ch);
            let mut r = FS_RECTF {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            };
            unsafe { b.FPDFText_GetLooseCharBox(tp, i, &mut r) };
            chars.push(TextChar {
                ch: ch.to_string(),
                x: r.left,
                y: r.bottom,
                w: r.right - r.left,
                h: r.top - r.bottom,
            });
        }
        (text, chars)
    }

    fn page_text(&self, id: DocId, index: u16) -> Result<PageText> {
        let d = self.doc(id)?;
        let p = self.page(d, index)?;
        let t = self.text_page(&p)?;
        let (text, chars) = self.chars_of(t.tp);
        Ok(PageText { index, text, chars })
    }

    fn search(&self, id: DocId, query: &str, case: bool, whole: bool) -> Result<Vec<SearchHit>> {
        let d = self.doc(id)?;
        let b = &self.b;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut needle: Vec<u16> = query.encode_utf16().collect();
        needle.push(0);
        let flags = (if case { 1 } else { 0 }) | (if whole { 2 } else { 0 });
        let count = unsafe { b.FPDF_GetPageCount(d.handle) }.max(0) as u16;
        let mut hits = Vec::new();
        for pi in 0..count {
            let p = self.page(d, pi)?;
            let Ok(t) = self.text_page(&p) else { continue };
            let (text, chars) = self.chars_of(t.tp);
            let text_chars: Vec<char> = text.chars().collect();
            let sh = unsafe { b.FPDFText_FindStart(t.tp, needle.as_ptr(), flags as std::os::raw::c_ulong, 0) };
            if sh.is_null() {
                continue;
            }
            while unsafe { b.FPDFText_FindNext(sh) } != 0 {
                let start = unsafe { b.FPDFText_GetSchResultIndex(sh) }.max(0) as usize;
                let len = unsafe { b.FPDFText_GetSchCount(sh) }.max(0) as usize;
                let ctx_s = start.saturating_sub(30);
                let ctx_e = (start + len + 30).min(text_chars.len());
                let context: String = text_chars[ctx_s..ctx_e]
                    .iter()
                    .map(|c| if c.is_control() { ' ' } else { *c })
                    .collect();
                hits.push(SearchHit {
                    page_index: pi,
                    start,
                    len,
                    context,
                    rects: line_rects(&chars[start..(start + len).min(chars.len())]),
                });
            }
            unsafe { b.FPDFText_FindClose(sh) };
        }
        Ok(hits)
    }

    // ----- outline and attachments -----

    fn outline(&self, id: DocId) -> Result<Vec<OutlineNode>> {
        let d = self.doc(id)?;
        let b = &self.b;
        fn walk(
            b: &dyn PdfiumLibraryBindings,
            doc: FPDF_DOCUMENT,
            mut bm: FPDF_BOOKMARK,
            depth: u32,
        ) -> Vec<OutlineNode> {
            let mut out = Vec::new();
            while !bm.is_null() && depth < 64 {
                let len = unsafe { b.FPDFBookmark_GetTitle(bm, std::ptr::null_mut(), 0) };
                let mut buf = vec![0u16; (len as usize) / 2];
                unsafe { b.FPDFBookmark_GetTitle(bm, buf.as_mut_ptr() as *mut c_void, len) };
                let dest = unsafe { b.FPDFBookmark_GetDest(doc, bm) };
                let page_index = if dest.is_null() {
                    None
                } else {
                    let i = unsafe { b.FPDFDest_GetDestPageIndex(doc, dest) };
                    (i >= 0).then_some(i as u16)
                };
                let child = unsafe { b.FPDFBookmark_GetFirstChild(doc, bm) };
                out.push(OutlineNode {
                    title: utf16_to_string(&buf),
                    page_index,
                    children: walk(b, doc, child, depth + 1),
                });
                bm = unsafe { b.FPDFBookmark_GetNextSibling(doc, bm) };
            }
            out
        }
        let root = unsafe { b.FPDFBookmark_GetFirstChild(d.handle, std::ptr::null_mut()) };
        Ok(walk(b.as_ref(), d.handle, root, 0))
    }

    fn attachment_bytes(&self, id: DocId, index: u32) -> Result<Vec<u8>> {
        let d = self.doc(id)?;
        let b = &self.b;
        let a = unsafe { b.FPDFDoc_GetAttachment(d.handle, index as i32) };
        if a.is_null() {
            return Err(SheafError::Pdf(format!("no attachment {index}")));
        }
        let mut size: std::os::raw::c_ulong = 0;
        unsafe { b.FPDFAttachment_GetFile(a, std::ptr::null_mut(), 0, &mut size) };
        let mut buf = vec![0u8; size as usize];
        let mut got: std::os::raw::c_ulong = 0;
        let ok =
            unsafe { b.FPDFAttachment_GetFile(a, buf.as_mut_ptr() as *mut c_void, size, &mut got) };
        if ok == 0 {
            return Err(SheafError::Pdf("could not read attachment".into()));
        }
        buf.truncate(got as usize);
        Ok(buf)
    }

    // ----- annotations -----

    fn annot_string(&self, a: FPDF_ANNOTATION, key: &str) -> String {
        let b = &self.b;
        let len = unsafe { b.FPDFAnnot_GetStringValue(a, key, std::ptr::null_mut(), 0) };
        if len <= 2 {
            return String::new();
        }
        let mut buf = vec![0u16; (len as usize) / 2];
        unsafe { b.FPDFAnnot_GetStringValue(a, key, buf.as_mut_ptr(), len) };
        utf16_to_string(&buf)
    }

    fn read_annotation(&self, page_index: u16, a: FPDF_ANNOTATION, index: u32) -> Annotation {
        let b = &self.b;
        let kind = AnnotKind::from_subtype(unsafe { b.FPDFAnnot_GetSubtype(a) });
        let mut r = FS_RECTF {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        unsafe { b.FPDFAnnot_GetRect(a, &mut r) };
        // PDFium refuses FPDFAnnot_GetColor once an appearance stream exists
        // (which happens after the first render), so we mirror colors into
        // private keys and prefer those.
        let color = |ty: FPDFANNOT_COLORTYPE, key: &str| {
            if let Some(c) = parse_color(&self.annot_string(a, key)) {
                return Some(c);
            }
            let (mut cr, mut cg, mut cb, mut ca) = (0u32, 0u32, 0u32, 0u32);
            let ok = unsafe { b.FPDFAnnot_GetColor(a, ty, &mut cr, &mut cg, &mut cb, &mut ca) };
            (ok != 0).then_some(Color {
                r: cr as u8,
                g: cg as u8,
                b: cb as u8,
            })
        };
        let (mut hr, mut vr, mut bw) = (0f32, 0f32, 1f32);
        unsafe { b.FPDFAnnot_GetBorder(a, &mut hr, &mut vr, &mut bw) };
        let quads = {
            let n = unsafe { b.FPDFAnnot_CountAttachmentPoints(a) };
            (0..n)
                .filter_map(|i| {
                    let mut q: FS_QUADPOINTSF = unsafe { std::mem::zeroed() };
                    (unsafe { b.FPDFAnnot_GetAttachmentPoints(a, i, &mut q) } != 0)
                        .then_some([q.x1, q.y1, q.x2, q.y2, q.x3, q.y3, q.x4, q.y4])
                })
                .collect()
        };
        let ink = {
            let n = unsafe { b.FPDFAnnot_GetInkListCount(a) };
            (0..n)
                .map(|i| {
                    let len = unsafe { b.FPDFAnnot_GetInkListPath(a, i, std::ptr::null_mut(), 0) };
                    let mut pts = vec![FS_POINTF { x: 0.0, y: 0.0 }; len as usize];
                    unsafe { b.FPDFAnnot_GetInkListPath(a, i, pts.as_mut_ptr(), len) };
                    pts.iter().map(|p| [p.x, p.y]).collect()
                })
                .collect()
        };
        let flags = unsafe { b.FPDFAnnot_GetFlags(a) } as u32;
        Annotation {
            page_index,
            index,
            kind,
            rect: Rect {
                x: r.left.min(r.right),
                y: r.bottom.min(r.top),
                w: (r.right - r.left).abs(),
                h: (r.top - r.bottom).abs(),
            },
            contents: self.annot_string(a, "Contents"),
            author: self.annot_string(a, "T"),
            subject: self.annot_string(a, "Subj"),
            modified: self.annot_string(a, "M"),
            color: color(FPDFANNOT_COLORTYPE_FPDFANNOT_COLORTYPE_Color, "SheafC"),
            interior_color: color(
                FPDFANNOT_COLORTYPE_FPDFANNOT_COLORTYPE_InteriorColor,
                "SheafIC",
            ),
            border_width: bw,
            quads,
            ink,
            hidden: flags & FPDF_ANNOT_FLAG_HIDDEN != 0,
            editable: !matches!(
                kind,
                AnnotKind::Widget | AnnotKind::Link | AnnotKind::Popup | AnnotKind::Other
            ),
        }
    }

    fn list_annotations(&self, id: DocId, page: u16) -> Result<Vec<Annotation>> {
        let d = self.doc(id)?;
        let b = &self.b;
        let p = self.page(d, page)?;
        let n = unsafe { b.FPDFPage_GetAnnotCount(p.page) }.max(0);
        let mut out = Vec::new();
        for i in 0..n {
            let a = unsafe { b.FPDFPage_GetAnnot(p.page, i) };
            if a.is_null() {
                continue;
            }
            let ann = self.read_annotation(page, a, i as u32);
            unsafe { b.FPDFPage_CloseAnnot(a) };
            if ann.kind != AnnotKind::Popup {
                out.push(ann);
            }
        }
        Ok(out)
    }

    /// Serialize the current document to bytes (full rewrite, no incremental section).
    fn snapshot(&self, handle: FPDF_DOCUMENT, flags: u32) -> Result<Vec<u8>> {
        // `base` must be the first field so PDFium's `pThis` pointer can be
        // cast back to the whole Writer.
        #[repr(C)]
        struct Writer {
            base: FPDF_FILEWRITE,
            buf: Vec<u8>,
        }
        unsafe extern "C" fn write_block(
            this: *mut FPDF_FILEWRITE,
            data: *const c_void,
            size: std::os::raw::c_ulong,
        ) -> i32 {
            let w = unsafe { &mut *(this as *mut Writer) };
            let s = unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) };
            w.buf.extend_from_slice(s);
            1
        }
        let mut w = Writer {
            base: FPDF_FILEWRITE {
                version: 1,
                WriteBlock: Some(write_block),
            },
            buf: Vec::new(),
        };
        let ok = unsafe {
            self.b
                .FPDF_SaveAsCopy(handle, &mut w.base as *mut FPDF_FILEWRITE, flags as std::os::raw::c_ulong)
        };
        if ok == 0 {
            return Err(SheafError::Pdf(
                "PDFium failed to serialize the document".into(),
            ));
        }
        Ok(w.buf)
    }

    /// Bytes for the document as it stands. Verbatim when nothing changed in
    /// PDFium; otherwise an incremental update for signed documents (so the
    /// existing signatures keep covering their byte ranges) or a compact
    /// rewrite for everything else.
    fn serialize(&self, d: &OpenDoc) -> Result<Vec<u8>> {
        if d.authoritative {
            return Ok(d.bytes.clone());
        }
        let flags = if has_signatures(&d.bytes) {
            FPDF_INCREMENTAL
        } else {
            FPDF_NO_INCREMENTAL
        };
        self.snapshot(d.handle, flags)
    }

    /// Push the current state to the undo stack before a mutation.
    fn checkpoint(&mut self, id: DocId) -> Result<()> {
        let snap = {
            let d = self.doc(id)?;
            self.serialize(d)?
        };
        let d = self.doc_mut(id)?;
        d.undo.push(snap);
        if d.undo.len() > MAX_UNDO {
            d.undo.remove(0);
        }
        d.redo.clear();
        d.modified = true;
        d.authoritative = false;
        Ok(())
    }

    /// Replace the live document with `bytes` (used by undo/redo/save).
    fn reload(&mut self, id: DocId, bytes: Vec<u8>) -> Result<()> {
        let password = self.doc(id)?.password.clone();
        let (bytes, handle, form, form_info) = self.load(bytes, password.as_deref())?;
        let d = self.doc_mut(id)?;
        let path = d.path.clone();
        let password = d.password.clone();
        let undo = std::mem::take(&mut d.undo);
        let redo = std::mem::take(&mut d.redo);
        let modified = d.modified;
        let old = std::mem::replace(
            d,
            OpenDoc {
                bytes,
                handle,
                form,
                form_info,
                path,
                password,
                undo,
                redo,
                modified,
                authoritative: true,
            },
        );
        self.free(old);
        Ok(())
    }

    fn undo_redo(&mut self, id: DocId, undo: bool) -> Result<DocumentInfo> {
        let current = {
            let d = self.doc(id)?;
            self.serialize(d)?
        };
        let target = {
            let d = self.doc_mut(id)?;
            let (from, to) = if undo {
                (&mut d.undo, &mut d.redo)
            } else {
                (&mut d.redo, &mut d.undo)
            };
            let Some(t) = from.pop() else {
                return self.info(id);
            };
            to.push(current);
            d.modified = true;
            t
        };
        self.reload(id, target)?;
        self.info(id)
    }

    fn apply_spec_colors(&self, a: FPDF_ANNOTATION, color: Option<Color>, interior: Option<Color>) {
        let b = &self.b;
        if color.is_some() || interior.is_some() {
            // Remove any generated appearance so PDFium accepts the color
            // change and regenerates the appearance on next render. A null
            // value deletes the AP entry; an empty string would leave an empty
            // stream behind, which blocks regeneration.
            unsafe {
                b.FPDFAnnot_SetAP(a, FPDF_ANNOT_APPEARANCEMODE_NORMAL as i32, std::ptr::null())
            };
        }
        if let Some(c) = color {
            unsafe { b.FPDFAnnot_SetStringValue_str(a, "SheafC", &fmt_color(c)) };
            unsafe {
                b.FPDFAnnot_SetColor(
                    a,
                    FPDFANNOT_COLORTYPE_FPDFANNOT_COLORTYPE_Color,
                    c.r as u32,
                    c.g as u32,
                    c.b as u32,
                    255,
                )
            };
        }
        if let Some(c) = interior {
            unsafe { b.FPDFAnnot_SetStringValue_str(a, "SheafIC", &fmt_color(c)) };
            unsafe {
                b.FPDFAnnot_SetColor(
                    a,
                    FPDFANNOT_COLORTYPE_FPDFANNOT_COLORTYPE_InteriorColor,
                    c.r as u32,
                    c.g as u32,
                    c.b as u32,
                    255,
                )
            };
        }
    }

    fn add_annotation(&mut self, id: DocId, page: u16, spec: AnnotationSpec) -> Result<Annotation> {
        let Some(subtype) = spec.kind.subtype() else {
            return Err(SheafError::Pdf(format!(
                "cannot create {:?} annotations",
                spec.kind
            )));
        };
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = &self.b;
        let p = self.page(d, page)?;
        let a = unsafe { b.FPDFPage_CreateAnnot(p.page, subtype) };
        if a.is_null() {
            return Err(SheafError::Pdf(
                "PDFium refused to create the annotation".into(),
            ));
        }
        let r = spec.rect;
        let rect = FS_RECTF {
            left: r.x,
            bottom: r.y,
            right: r.x + r.w,
            top: r.y + r.h,
        };
        unsafe {
            b.FPDFAnnot_SetRect(a, &rect);
            b.FPDFAnnot_SetFlags(a, FPDF_ANNOT_FLAG_PRINT as i32);
            b.FPDFAnnot_SetStringValue_str(a, "Contents", &spec.contents);
            b.FPDFAnnot_SetStringValue_str(a, "T", &spec.author);
            b.FPDFAnnot_SetStringValue_str(a, "M", &pdf_date_now());
            b.FPDFAnnot_SetStringValue_str(a, "NM", &format!("sheaf-{}", nanos()));
        }
        self.apply_spec_colors(a, spec.color, spec.interior_color);
        match spec.kind {
            AnnotKind::Highlight
            | AnnotKind::Underline
            | AnnotKind::StrikeOut
            | AnnotKind::Squiggly => {
                for q in &spec.quads {
                    let qp = FS_QUADPOINTSF {
                        x1: q[0],
                        y1: q[1],
                        x2: q[2],
                        y2: q[3],
                        x3: q[4],
                        y3: q[5],
                        x4: q[6],
                        y4: q[7],
                    };
                    unsafe { b.FPDFAnnot_AppendAttachmentPoints(a, &qp) };
                }
            }
            AnnotKind::Ink => {
                unsafe { b.FPDFAnnot_SetBorder(a, 0.0, 0.0, spec.border_width) };
                for path in &spec.ink {
                    let pts: Vec<FS_POINTF> = path
                        .iter()
                        .map(|p| FS_POINTF { x: p[0], y: p[1] })
                        .collect();
                    unsafe { b.FPDFAnnot_AddInkStroke(a, pts.as_ptr(), pts.len()) };
                }
            }
            AnnotKind::Square | AnnotKind::Circle => {
                unsafe { b.FPDFAnnot_SetBorder(a, 0.0, 0.0, spec.border_width) };
            }
            AnnotKind::Text => unsafe {
                b.FPDFAnnot_SetStringValue_str(a, "Name", "Comment");
                b.FPDFAnnot_SetFlags(
                    a,
                    (FPDF_ANNOT_FLAG_PRINT | FPDF_ANNOT_FLAG_NOZOOM | FPDF_ANNOT_FLAG_NOROTATE)
                        as i32,
                );
                b.FPDFAnnot_SetAP_str(
                    a,
                    FPDF_ANNOT_APPEARANCEMODE_NORMAL as i32,
                    &note_appearance(r, spec.color),
                );
            },
            AnnotKind::FreeText => {
                let c = spec.color.unwrap_or(Color { r: 0, g: 0, b: 0 });
                unsafe {
                    b.FPDFAnnot_SetStringValue_str(
                        a,
                        "DA",
                        &format!(
                            "/Helv {} Tf {} {} {} rg",
                            spec.font_size,
                            f(c.r),
                            f(c.g),
                            f(c.b)
                        ),
                    );
                    b.FPDFAnnot_SetAP_str(
                        a,
                        FPDF_ANNOT_APPEARANCEMODE_NORMAL as i32,
                        &free_text_appearance(r, &spec.contents, spec.font_size, c),
                    );
                }
            }
            _ => {}
        }
        let index = unsafe { b.FPDFPage_GetAnnotIndex(p.page, a) }.max(0) as u32;
        let ann = self.read_annotation(page, a, index);
        unsafe {
            b.FPDFPage_CloseAnnot(a);
            b.FPDFPage_GenerateContent(p.page);
        }
        Ok(ann)
    }

    fn update_annotation(
        &mut self,
        id: DocId,
        page: u16,
        index: u32,
        patch: AnnotationPatch,
    ) -> Result<Annotation> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = &self.b;
        let p = self.page(d, page)?;
        let a = unsafe { b.FPDFPage_GetAnnot(p.page, index as i32) };
        if a.is_null() {
            return Err(SheafError::Pdf(format!(
                "no annotation {index} on page {page}"
            )));
        }
        let kind = AnnotKind::from_subtype(unsafe { b.FPDFAnnot_GetSubtype(a) });
        if let Some(r) = patch.rect {
            let rect = FS_RECTF {
                left: r.x,
                bottom: r.y,
                right: r.x + r.w,
                top: r.y + r.h,
            };
            unsafe { b.FPDFAnnot_SetRect(a, &rect) };
        }
        if let Some(c) = &patch.contents {
            unsafe { b.FPDFAnnot_SetStringValue_str(a, "Contents", c) };
        }
        if let Some(t) = &patch.author {
            unsafe { b.FPDFAnnot_SetStringValue_str(a, "T", t) };
        }
        self.apply_spec_colors(a, patch.color, patch.interior_color);
        if let Some(w) = patch.border_width {
            unsafe { b.FPDFAnnot_SetBorder(a, 0.0, 0.0, w) };
        }
        if let Some(h) = patch.hidden {
            let mut flags = unsafe { b.FPDFAnnot_GetFlags(a) } as u32;
            if h {
                flags |= FPDF_ANNOT_FLAG_HIDDEN
            } else {
                flags &= !FPDF_ANNOT_FLAG_HIDDEN
            }
            unsafe { b.FPDFAnnot_SetFlags(a, flags as i32) };
        }
        unsafe { b.FPDFAnnot_SetStringValue_str(a, "M", &pdf_date_now()) };
        // Regenerate our own appearance streams for the kinds PDFium will not.
        let current = self.read_annotation(page, a, index);
        match kind {
            AnnotKind::Text => unsafe {
                b.FPDFAnnot_SetAP_str(
                    a,
                    FPDF_ANNOT_APPEARANCEMODE_NORMAL as i32,
                    &note_appearance(current.rect, current.color),
                );
            },
            AnnotKind::FreeText => {
                let c = current.color.unwrap_or(Color { r: 0, g: 0, b: 0 });
                let da = self.annot_string(a, "DA");
                let size = da
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(12.0);
                unsafe {
                    b.FPDFAnnot_SetStringValue_str(
                        a,
                        "DA",
                        &format!("/Helv {} Tf {} {} {} rg", size, f(c.r), f(c.g), f(c.b)),
                    );
                    b.FPDFAnnot_SetAP_str(
                        a,
                        FPDF_ANNOT_APPEARANCEMODE_NORMAL as i32,
                        &free_text_appearance(current.rect, &current.contents, size, c),
                    );
                }
            }
            // For PDFium-generated kinds, drop the stale AP so it regenerates on render.
            _ => unsafe {
                b.FPDFAnnot_SetAP(a, FPDF_ANNOT_APPEARANCEMODE_NORMAL as i32, std::ptr::null());
            },
        }
        let ann = self.read_annotation(page, a, index);
        unsafe {
            b.FPDFPage_CloseAnnot(a);
            b.FPDFPage_GenerateContent(p.page);
        }
        Ok(ann)
    }

    fn delete_annotation(&mut self, id: DocId, page: u16, index: u32) -> Result<()> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = &self.b;
        let p = self.page(d, page)?;
        let ok = unsafe { b.FPDFPage_RemoveAnnot(p.page, index as i32) };
        if ok == 0 {
            return Err(SheafError::Pdf(format!(
                "could not delete annotation {index}"
            )));
        }
        unsafe { b.FPDFPage_GenerateContent(p.page) };
        Ok(())
    }

    // ----- forms -----

    /// UTF-16 string via the (buf, buflen)->len pattern shared by the
    /// FPDFAnnot_GetFormField* getters.
    fn form_string(
        &self,
        get: impl Fn(*mut FPDF_WCHAR, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
    ) -> String {
        let len = get(std::ptr::null_mut(), 0);
        if len <= 2 {
            return String::new();
        }
        let mut buf = vec![0u16; (len as usize) / 2];
        get(buf.as_mut_ptr(), len);
        utf16_to_string(&buf)
    }

    fn read_form_field(
        &self,
        d: &OpenDoc,
        page_index: u16,
        a: FPDF_ANNOTATION,
        annot_index: u32,
    ) -> FormField {
        let b = &self.b;
        let form = d.form;
        let ty = unsafe { b.FPDFAnnot_GetFormFieldType(form, a) };
        let kind = match ty {
            FPDF_FORMFIELD_PUSHBUTTON => "button",
            FPDF_FORMFIELD_CHECKBOX => "checkbox",
            FPDF_FORMFIELD_RADIOBUTTON => "radio",
            FPDF_FORMFIELD_COMBOBOX => "combo",
            FPDF_FORMFIELD_LISTBOX => "listbox",
            FPDF_FORMFIELD_TEXTFIELD => "text",
            FPDF_FORMFIELD_SIGNATURE => "signature",
            _ => "unknown",
        };
        let mut r = FS_RECTF {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        unsafe { b.FPDFAnnot_GetRect(a, &mut r) };
        let flags = unsafe { b.FPDFAnnot_GetFormFieldFlags(form, a) } as u32;
        let name =
            self.form_string(|buf, len| unsafe { b.FPDFAnnot_GetFormFieldName(form, a, buf, len) });
        let alt_name = self.form_string(|buf, len| unsafe {
            b.FPDFAnnot_GetFormFieldAlternateName(form, a, buf, len)
        });
        let value = self
            .form_string(|buf, len| unsafe { b.FPDFAnnot_GetFormFieldValue(form, a, buf, len) });
        let export_value = self.form_string(|buf, len| unsafe {
            b.FPDFAnnot_GetFormFieldExportValue(form, a, buf, len)
        });
        let checked = matches!(ty, FPDF_FORMFIELD_CHECKBOX | FPDF_FORMFIELD_RADIOBUTTON)
            && unsafe { b.FPDFAnnot_IsChecked(form, a) } != 0;
        let options = if matches!(ty, FPDF_FORMFIELD_COMBOBOX | FPDF_FORMFIELD_LISTBOX) {
            let n = unsafe { b.FPDFAnnot_GetOptionCount(form, a) }.max(0);
            (0..n)
                .map(|i| FormFieldOption {
                    label: self.form_string(|buf, len| unsafe {
                        b.FPDFAnnot_GetOptionLabel(form, a, i, buf, len)
                    }),
                    selected: unsafe { b.FPDFAnnot_IsOptionSelected(form, a, i) } != 0,
                })
                .collect()
        } else {
            Vec::new()
        };
        FormField {
            page_index,
            annot_index,
            name,
            alt_name,
            kind: kind.into(),
            value,
            rect: Rect {
                x: r.left,
                y: r.bottom.min(r.top),
                w: (r.right - r.left).abs(),
                h: (r.top - r.bottom).abs(),
            },
            readonly: flags & FIELD_FLAG_READONLY != 0,
            required: flags & FIELD_FLAG_REQUIRED != 0,
            multiline: flags & FIELD_FLAG_MULTILINE != 0,
            password: flags & FIELD_FLAG_PASSWORD != 0,
            multiselect: flags & FIELD_FLAG_MULTISELECT != 0,
            export_value,
            checked,
            options,
        }
    }

    fn list_form_fields(&self, id: DocId, page: u16) -> Result<Vec<FormField>> {
        let d = self.doc(id)?;
        if d.form.is_null() {
            return Ok(Vec::new());
        }
        let b = &self.b;
        let p = self.page(d, page)?;
        let n = unsafe { b.FPDFPage_GetAnnotCount(p.page) }.max(0);
        let mut out = Vec::new();
        for i in 0..n {
            let a = unsafe { b.FPDFPage_GetAnnot(p.page, i) };
            if a.is_null() {
                continue;
            }
            let is_widget = unsafe { b.FPDFAnnot_GetSubtype(a) } == FPDF_ANNOT_WIDGET as i32;
            if is_widget {
                let ty = unsafe { b.FPDFAnnot_GetFormFieldType(d.form, a) };
                if ty != FPDF_FORMFIELD_PUSHBUTTON {
                    out.push(self.read_form_field(d, page, a, i as u32));
                }
            }
            unsafe { b.FPDFPage_CloseAnnot(a) };
        }
        Ok(out)
    }

    /// Set a widget's value through the form-fill environment so PDFium
    /// regenerates appearance streams and keeps radio groups consistent.
    fn set_form_field_value(
        &mut self,
        id: DocId,
        page: u16,
        annot_index: u32,
        value: &str,
    ) -> Result<FormField> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        if d.form.is_null() {
            return Err(SheafError::Pdf("document has no form".into()));
        }
        let b = &self.b;
        let p = self.page(d, page)?;
        let a = unsafe { b.FPDFPage_GetAnnot(p.page, annot_index as i32) };
        if a.is_null() {
            return Err(SheafError::Pdf(format!(
                "no annotation {annot_index} on page {page}"
            )));
        }
        let close = |a| unsafe { b.FPDFPage_CloseAnnot(a) };
        let ty = unsafe { b.FPDFAnnot_GetFormFieldType(d.form, a) };
        let flags = unsafe { b.FPDFAnnot_GetFormFieldFlags(d.form, a) } as u32;
        if flags & FIELD_FLAG_READONLY != 0 {
            close(a);
            return Err(SheafError::Pdf("field is read-only".into()));
        }
        let focus_ok = unsafe { b.FORM_SetFocusedAnnot(d.form, a) } != 0;
        if !focus_ok {
            close(a);
            return Err(SheafError::Pdf("could not focus form field".into()));
        }
        match ty {
            FPDF_FORMFIELD_TEXTFIELD => unsafe {
                b.FORM_SelectAllText(d.form, p.page);
                let mut wide: Vec<u16> = value.encode_utf16().collect();
                wide.push(0);
                b.FORM_ReplaceSelection(d.form, p.page, wide.as_ptr());
            },
            FPDF_FORMFIELD_COMBOBOX => unsafe {
                // Prefer selecting a matching option (works for non-editable
                // combos); fall back to typing for editable ones.
                let n = b.FPDFAnnot_GetOptionCount(d.form, a).max(0);
                let mut matched = false;
                for i in 0..n {
                    let label = self
                        .form_string(|buf, len| b.FPDFAnnot_GetOptionLabel(d.form, a, i, buf, len));
                    if label.eq_ignore_ascii_case(value) {
                        b.FORM_SetIndexSelected(d.form, p.page, i, 1);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    b.FORM_SelectAllText(d.form, p.page);
                    let mut wide: Vec<u16> = value.encode_utf16().collect();
                    wide.push(0);
                    b.FORM_ReplaceSelection(d.form, p.page, wide.as_ptr());
                }
            },
            FPDF_FORMFIELD_CHECKBOX | FPDF_FORMFIELD_RADIOBUTTON => unsafe {
                // Toggling happens through a click cycle at the widget center.
                let mut r = FS_RECTF {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                };
                b.FPDFAnnot_GetRect(a, &mut r);
                let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
                let already = b.FPDFAnnot_IsChecked(d.form, a) != 0;
                let want = value != "off";
                if already != want {
                    b.FORM_OnLButtonDown(d.form, p.page, 0, cx.into(), cy.into());
                    b.FORM_OnLButtonUp(d.form, p.page, 0, cx.into(), cy.into());
                }
            },
            FPDF_FORMFIELD_LISTBOX => unsafe {
                let wanted: Vec<&str> = value.split('\n').filter(|s| !s.is_empty()).collect();
                let n = b.FPDFAnnot_GetOptionCount(d.form, a).max(0);
                for i in 0..n {
                    let label = self
                        .form_string(|buf, len| b.FPDFAnnot_GetOptionLabel(d.form, a, i, buf, len));
                    b.FORM_SetIndexSelected(
                        d.form,
                        p.page,
                        i,
                        wanted.contains(&label.as_str()) as i32,
                    );
                }
            },
            _ => {
                close(a);
                return Err(SheafError::Pdf("field type cannot be set".into()));
            }
        }
        // Commit: kill focus so PDFium flushes the value into the field dict.
        unsafe { b.FORM_ForceToKillFocus(d.form) };
        let field = self.read_form_field(d, page, a, annot_index);
        close(a);
        unsafe { b.FPDFPage_GenerateContent(p.page) };
        Ok(field)
    }

    fn all_form_fields(&self, id: DocId) -> Result<Vec<FormField>> {
        let d = self.doc(id)?;
        let count = unsafe { self.b.FPDF_GetPageCount(d.handle) }.max(0) as u16;
        let mut out = Vec::new();
        for i in 0..count {
            out.extend(self.list_form_fields(id, i)?);
        }
        Ok(out)
    }

    fn export_xfdf(&self, id: DocId) -> Result<String> {
        let d = self.doc(id)?;
        let href = xml_escape(&d.path.to_string_lossy());
        let fields = self.all_form_fields(id)?;
        // Group widgets by fully qualified name; radio groups share one name.
        let mut by_name: Vec<(String, String)> = Vec::new();
        for f in &fields {
            if f.kind == "signature" || f.kind == "button" {
                continue;
            }
            let value = match f.kind.as_str() {
                "checkbox" | "radio" => {
                    if f.checked {
                        if f.export_value.is_empty() {
                            "On".to_string()
                        } else {
                            f.export_value.clone()
                        }
                    } else if f.kind == "radio" {
                        continue; // unchecked radios: the checked sibling wins
                    } else {
                        "Off".to_string()
                    }
                }
                "listbox" => f
                    .options
                    .iter()
                    .filter(|o| o.selected)
                    .map(|o| o.label.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => f.value.clone(),
            };
            match by_name.iter_mut().find(|(n, _)| *n == f.name) {
                Some(e) => e.1 = value,
                None => by_name.push((f.name.clone(), value)),
            }
        }
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n  <fields>\n",
        );
        for (name, value) in by_name {
            let values = if value.contains('\n') && name != "comments" {
                value
                    .split('\n')
                    .map(|v| format!("<value>{}</value>", xml_escape(v)))
                    .collect::<String>()
            } else {
                format!("<value>{}</value>", xml_escape(&value))
            };
            xml.push_str(&format!(
                "    <field name=\"{}\">{}</field>\n",
                xml_escape(&name),
                values
            ));
        }
        xml.push_str(&format!("  </fields>\n  <f href=\"{href}\"/>\n</xfdf>\n"));
        Ok(xml)
    }

    fn import_xfdf(&mut self, id: DocId, xfdf: &str) -> Result<u32> {
        let values = parse_xfdf_fields(xfdf);
        if values.is_empty() {
            return Err(SheafError::Pdf("no <field> entries found in XFDF".into()));
        }
        self.checkpoint(id)?;
        let fields = self.all_form_fields(id)?;
        let mut applied = 0u32;
        for f in &fields {
            let Some(wanted) = values.get(&f.name) else {
                continue;
            };
            let value = match f.kind.as_str() {
                "checkbox" => {
                    let on = !wanted.is_empty() && !wanted[0].eq_ignore_ascii_case("off");
                    if on { "on" } else { "off" }.to_string()
                }
                "radio" => {
                    let export = if f.export_value.is_empty() {
                        "On".to_string()
                    } else {
                        f.export_value.clone()
                    };
                    if wanted.first().map(|w| *w == export).unwrap_or(false) {
                        "on".to_string()
                    } else {
                        continue; // not this widget's export value
                    }
                }
                "listbox" => wanted.join("\n"),
                "text" | "combo" => wanted.first().cloned().unwrap_or_default(),
                _ => continue,
            };
            // checkpoint() already ran once; set values without stacking more.
            let d = self.doc(id)?;
            if d.form.is_null() {
                break;
            }
            if self
                .set_form_field_value_nocheckpoint(id, f.page_index, f.annot_index, &value)
                .is_ok()
            {
                applied += 1;
            }
        }
        Ok(applied)
    }

    fn set_form_field_value_nocheckpoint(
        &mut self,
        id: DocId,
        page: u16,
        annot_index: u32,
        value: &str,
    ) -> Result<FormField> {
        // Same as set_form_field_value but without pushing an undo snapshot;
        // used by XFDF import which checkpoints once for the whole batch.
        let d = self.doc(id)?;
        let undo_len = d.undo.len();
        let result = self.set_form_field_value(id, page, annot_index, value);
        if let Ok(d) = self.doc_mut(id) {
            while d.undo.len() > undo_len {
                d.undo.remove(undo_len);
            }
        }
        result
    }

    // ----- page organization -----

    fn info_after_pagechange(&mut self, id: DocId) -> Result<DocumentInfo> {
        // Widget caches inside the form environment can go stale across
        // structural changes; reload from a snapshot to be safe.
        let bytes = {
            let d = self.doc(id)?;
            self.snapshot(d.handle, FPDF_NO_INCREMENTAL)?
        };
        self.reload(id, bytes)?;
        self.info(id)
    }

    fn rotate_pages(&mut self, id: DocId, pages: &[u16], delta: i32) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = &self.b;
        let steps = delta.div_euclid(90).rem_euclid(4);
        for &i in pages {
            let p = self.page(d, i)?;
            let cur = unsafe { b.FPDFPage_GetRotation(p.page) };
            unsafe { b.FPDFPage_SetRotation(p.page, (cur + steps).rem_euclid(4)) };
        }
        self.info_after_pagechange(id)
    }

    fn delete_pages(&mut self, id: DocId, pages: &[u16]) -> Result<DocumentInfo> {
        let count = unsafe { self.b.FPDF_GetPageCount(self.doc(id)?.handle) } as usize;
        if pages.len() >= count {
            return Err(SheafError::Pdf("cannot delete every page".into()));
        }
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let mut sorted: Vec<u16> = pages.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        // Delete from the back so earlier indices stay valid.
        for &i in sorted.iter().rev() {
            unsafe { self.b.FPDFPage_Delete(d.handle, i as i32) };
        }
        self.info_after_pagechange(id)
    }

    fn move_pages(&mut self, id: DocId, pages: &[u16], dest: u16) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let idx: Vec<std::os::raw::c_int> = pages.iter().map(|&p| p as i32).collect();
        let ok = unsafe {
            self.b.FPDF_MovePages(
                d.handle,
                idx.as_ptr(),
                idx.len() as std::os::raw::c_ulong,
                dest as i32,
            )
        };
        if ok == 0 {
            // Roll the checkpoint back so a failed move is not an undo step.
            let d = self.doc_mut(id)?;
            d.undo.pop();
            return Err(SheafError::Pdf("could not move pages".into()));
        }
        self.info_after_pagechange(id)
    }

    fn insert_pages(
        &mut self,
        id: DocId,
        path: &Path,
        password: Option<&str>,
        at: u16,
    ) -> Result<DocumentInfo> {
        let bytes = std::fs::read(path)?;
        self.checkpoint(id)?;
        let b = &self.b;
        // Load the source document directly (no form environment needed).
        let src = unsafe { b.FPDF_LoadMemDocument64(&bytes, password) };
        if src.is_null() {
            let err = unsafe { b.FPDF_GetLastError() } as u32;
            let d = self.doc_mut(id)?;
            d.undo.pop();
            return Err(if err == FPDF_ERR_PASSWORD {
                SheafError::PasswordRequired
            } else {
                SheafError::Pdf(format!("could not open {} (error {err})", path.display()))
            });
        }
        let d = self.doc(id)?;
        let n = unsafe { b.FPDF_GetPageCount(src) };
        let indices: Vec<std::os::raw::c_int> = (0..n).collect();
        let ok = unsafe {
            b.FPDF_ImportPagesByIndex(
                d.handle,
                src,
                indices.as_ptr(),
                indices.len() as std::os::raw::c_ulong,
                at as i32,
            )
        };
        unsafe { b.FPDF_CloseDocument(src) };
        if ok == 0 {
            let d = self.doc_mut(id)?;
            d.undo.pop();
            return Err(SheafError::Pdf("could not import pages".into()));
        }
        self.info_after_pagechange(id)
    }

    fn extract_pages(&self, id: DocId, pages: &[u16], path: &Path) -> Result<()> {
        let d = self.doc(id)?;
        let b = &self.b;
        let dest = unsafe { b.FPDF_CreateNewDocument() };
        if dest.is_null() {
            return Err(SheafError::Pdf("could not create destination".into()));
        }
        let idx: Vec<std::os::raw::c_int> = pages.iter().map(|&p| p as i32).collect();
        let ok = unsafe {
            b.FPDF_ImportPagesByIndex(
                dest,
                d.handle,
                idx.as_ptr(),
                idx.len() as std::os::raw::c_ulong,
                0,
            )
        };
        if ok == 0 {
            unsafe { b.FPDF_CloseDocument(dest) };
            return Err(SheafError::Pdf("could not copy pages".into()));
        }
        let bytes = self.snapshot(dest, FPDF_NO_INCREMENTAL);
        unsafe { b.FPDF_CloseDocument(dest) };
        std::fs::write(path, bytes?)?;
        Ok(())
    }

    fn crop_pages(&mut self, id: DocId, pages: &[u16], bx: [f32; 4]) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = &self.b;
        for &i in pages {
            let p = self.page(d, i)?;
            unsafe { b.FPDFPage_SetCropBox(p.page, bx[0], bx[1], bx[2], bx[3]) };
        }
        self.info_after_pagechange(id)
    }

    fn stamp_pages(&mut self, id: DocId, spec: &StampSpec) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = &self.b;
        let total = unsafe { b.FPDF_GetPageCount(d.handle) } as u16;
        let targets: Vec<u16> = if spec.pages.is_empty() {
            (0..total).collect()
        } else {
            spec.pages.to_vec()
        };
        let margin = 28.0f32; // 28pt from the page edge (a bit under 1cm)
        for (i, &page_index) in targets.iter().enumerate() {
            let p = self.page(d, page_index)?;
            let (mut l, mut bo, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
            let got = unsafe { b.FPDFPage_GetCropBox(p.page, &mut l, &mut bo, &mut r, &mut t) };
            if got == 0 {
                unsafe { b.FPDFPage_GetMediaBox(p.page, &mut l, &mut bo, &mut r, &mut t) };
            }
            let (w, h) = (r - l, t - bo);
            let n = spec.start_at + i as u32;
            let text = spec
                .text
                .replace("{n}", &n.to_string())
                .replace("{total}", &total.to_string())
                .replace(
                    "{bates}",
                    &format!("{:0width$}", n, width = spec.bates_digits.max(1) as usize),
                );
            let obj = unsafe { b.FPDFPageObj_NewTextObj(d.handle, "Helvetica", spec.font_size) };
            if obj.is_null() {
                return Err(SheafError::Pdf("could not create text object".into()));
            }
            unsafe {
                b.FPDFText_SetText_str(obj, &text);
                b.FPDFPageObj_SetFillColor(
                    obj,
                    spec.color.r as u32,
                    spec.color.g as u32,
                    spec.color.b as u32,
                    spec.opacity as u32,
                );
            }
            // Rough text width for centering: Helvetica averages ~0.5em/char.
            let text_w = text.chars().count() as f32 * spec.font_size * 0.5;
            let (x, y, rotate) = match spec.position.as_str() {
                "header-left" => (l + margin, t - margin, false),
                "header-center" => (l + (w - text_w) / 2.0, t - margin, false),
                "header-right" => (r - margin - text_w, t - margin, false),
                "footer-left" => (l + margin, bo + margin - spec.font_size, false),
                "footer-center" => (l + (w - text_w) / 2.0, bo + margin - spec.font_size, false),
                "footer-right" => (r - margin - text_w, bo + margin - spec.font_size, false),
                "watermark" => (l + w / 2.0, bo + h / 2.0, true),
                other => {
                    return Err(SheafError::Pdf(format!("unknown stamp position {other}")));
                }
            };
            unsafe {
                if rotate {
                    // 45-degree diagonal centered on the page.
                    let (s, c) = (45f32.to_radians().sin(), 45f32.to_radians().cos());
                    let (hx, hy) = (text_w / 2.0, spec.font_size / 2.0);
                    b.FPDFPageObj_Transform(
                        obj,
                        c as f64,
                        s as f64,
                        (-s) as f64,
                        c as f64,
                        (x - hx * c + hy * s) as f64,
                        (y - hx * s - hy * c) as f64,
                    );
                } else {
                    b.FPDFPageObj_Transform(obj, 1.0, 0.0, 0.0, 1.0, x as f64, y as f64);
                }
                b.FPDFPage_InsertObject(p.page, obj);
                b.FPDFPage_GenerateContent(p.page);
            }
        }
        self.info_after_pagechange(id)
    }

    // ----- save -----


    // ----- M5 security -----

    /// Current on-disk-equivalent bytes: unsaved PDFium edits are serialized
    /// first so the signature covers what the user sees. Signing an
    /// unmodified document uses the original bytes so earlier signatures
    /// remain byte-for-byte intact.
    fn current_bytes(&self, id: DocId) -> Result<Vec<u8>> {
        let d = self.doc(id)?;
        self.serialize(d)
    }

    fn sign_document(
        &mut self,
        id: DocId,
        identities_dir: PathBuf,
        spec: &crate::security::SignSpec,
    ) -> Result<DocumentInfo> {
        let bytes = self.current_bytes(id)?;
        let password = self.doc(id)?.password.clone();
        let store = crate::security::IdentityStore::new(identities_dir);
        let signed = crate::security::sign(&bytes, password.as_deref(), &store, spec)?;
        self.checkpoint(id)?;
        self.reload(id, signed)?;
        self.info(id)
    }

    fn list_signatures(&self, id: DocId) -> Result<Vec<crate::security::SignatureInfo>> {
        let d = self.doc(id)?;
        // Verify against the bytes as PDFium holds them (what was opened or
        // last saved/signed); unsaved annotation edits are not on disk yet.
        crate::security::list_signatures(&d.bytes, d.password.as_deref())
    }

    fn protect(&mut self, id: DocId, spec: &crate::security::SecuritySpec) -> Result<DocumentInfo> {
        let bytes = self.current_bytes(id)?;
        let password = self.doc(id)?.password.clone();
        let out = crate::security::protect(&bytes, password.as_deref(), spec)?;
        self.checkpoint(id)?;
        // The new file opens with the user password (or none). Switch the
        // stored password so reload/undo/save keep working.
        let open_pw = if spec.user_password.is_empty() {
            if spec.owner_password.is_empty() {
                None
            } else {
                Some(spec.owner_password.clone())
            }
        } else {
            Some(spec.user_password.clone())
        };
        self.doc_mut(id)?.password = open_pw;
        self.reload(id, out)?;
        self.info(id)
    }

    fn unprotect(&mut self, id: DocId) -> Result<DocumentInfo> {
        let bytes = self.current_bytes(id)?;
        let password = self.doc(id)?.password.clone();
        let out = crate::security::unprotect(&bytes, password.as_deref())?;
        self.checkpoint(id)?;
        self.doc_mut(id)?.password = None;
        self.reload(id, out)?;
        self.info(id)
    }

    fn save(&mut self, id: DocId, opts: SaveOptions) -> Result<DocumentInfo> {
        let target = match &opts.path {
            Some(p) => PathBuf::from(p),
            None => self.doc(id)?.path.clone(),
        };
        let bytes = {
            let d = self.doc(id)?;
            let b = &self.b;
            if opts.flatten {
                let count = unsafe { b.FPDF_GetPageCount(d.handle) }.max(0) as u16;
                for i in 0..count {
                    let p = self.page(d, i)?;
                    unsafe { b.FPDFPage_Flatten(p.page, FLAT_NORMALDISPLAY as i32) };
                }
                self.snapshot(d.handle, FPDF_NO_INCREMENTAL)?
            } else {
                self.serialize(d)?
            }
        };
        let tmp = target.with_extension("pdf.sheaf-tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &target).or_else(|_| {
            std::fs::copy(&tmp, &target)
                .map(|_| ())
                .and_then(|_| std::fs::remove_file(&tmp))
        })?;
        // Reload from the saved bytes so PDFium's state matches disk exactly.
        self.reload(id, bytes)?;
        let d = self.doc_mut(id)?;
        d.path = target;
        d.modified = false;
        if opts.flatten {
            d.undo.clear();
            d.redo.clear();
        }
        self.info(id)
    }
}

// ---------- helpers ----------

/// Cheap structural probe: a signed PDF carries a /ByteRange in a /Sig dict.
fn has_signatures(bytes: &[u8]) -> bool {
    bytes.windows(10).any(|w| w == b"/ByteRange")
}

fn utf16_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Minimal XFDF reader: extracts `<field name="...">(<value>...</value>)+`
/// pairs, including Adobe's nested-field form (`<field><field>...`), by
/// tracking the name path. Tolerates attributes and whitespace; ignores
/// everything outside `<fields>`.
fn parse_xfdf_fields(xml: &str) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut path: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        rest = &rest[gt + 1..];
        let tag_trim = tag.trim();
        if let Some(field_tag) = tag_trim.strip_prefix("field") {
            // Exclude <fields>/</fields> (prefix collision) and </field>.
            if field_tag.starts_with('s') {
                continue;
            }
            let self_closing = field_tag.trim_end().ends_with('/');
            let name = field_tag
                .split("name=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(xml_unescape)
                .unwrap_or_default();
            if !self_closing {
                path.push(name);
            }
        } else if tag_trim == "/field" {
            path.pop();
        } else if tag_trim.starts_with("value") && !tag_trim.ends_with('/') {
            if let Some(end) = rest.find("</value>") {
                let value = xml_unescape(rest[..end].trim());
                rest = &rest[end + 8..];
                if !path.is_empty() {
                    out.entry(path.join(".")).or_default().push(value);
                }
            }
        }
    }
    out
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// PDF date string (D:YYYYMMDDHHmmSSZ) for now, UTC.
fn pdf_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("D:{y:04}{mo:02}{d:02}{h:02}{m:02}{s:02}Z")
}

fn fmt_color(c: Color) -> String {
    format!("{},{},{}", c.r, c.g, c.b)
}

fn parse_color(s: &str) -> Option<Color> {
    let mut it = s.split(',').map(|p| p.trim().parse::<u8>().ok());
    Some(Color {
        r: it.next()??,
        g: it.next()??,
        b: it.next()??,
    })
}

fn f(c: u8) -> String {
    format!("{:.3}", c as f32 / 255.0)
}

/// Group consecutive character boxes into one rectangle per text line.
pub fn line_rects(chars: &[TextChar]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for c in chars.iter().filter(|c| c.w > 0.0 && c.h > 0.0) {
        let mid = c.y + c.h / 2.0;
        if let Some(last) = out.last_mut() {
            if mid >= last.y && mid <= last.y + last.h {
                let x0 = last.x.min(c.x);
                let y0 = last.y.min(c.y);
                let x1 = (last.x + last.w).max(c.x + c.w);
                let y1 = (last.y + last.h).max(c.y + c.h);
                *last = Rect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                };
                continue;
            }
        }
        out.push(Rect {
            x: c.x,
            y: c.y,
            w: c.w,
            h: c.h,
        });
    }
    out
}

/// Appearance stream for a sticky note icon: a rounded speech-bubble outline
/// in the stroke color with a light fill. Drawn in the annotation's own
/// coordinate space (BBox = Rect), so we translate to the rect origin.
fn note_appearance(r: Rect, color: Option<Color>) -> String {
    let c = color.unwrap_or(Color {
        r: 255,
        g: 200,
        b: 0,
    });
    let (x, y, w, h) = (r.x, r.y, r.w.max(4.0), r.h.max(4.0));
    let rad = (w.min(h) * 0.2).max(1.0);
    let (x0, y0, x1, y1) = (x + 1.0, y + 1.0, x + w - 1.0, y + h - 1.0);
    format!(
        "q {sr} {sg} {sb} RG 1 1 0.85 rg 1 w \
         {x0p} {y0} m {x1m} {y0} l {x1m} {y0} {x1} {y0} {x1} {y0p} c \
         {x1} {y1m} l {x1} {y1m} {x1} {y1} {x1m} {y1} c \
         {x0p} {y1} l {x0p} {y1} {x0} {y1} {x0} {y1m} c \
         {x0} {y0p} l {x0} {y0p} {x0} {y0} {x0p} {y0} c h B \
         {lx0} {ly1} m {lx1} {ly1} l S {lx0} {ly2} m {lx1} {ly2} l S {lx0} {ly3} m {lx2} {ly3} l S Q",
        sr = f(c.r),
        sg = f(c.g),
        sb = f(c.b),
        x0p = x0 + rad,
        x1m = x1 - rad,
        y0p = y0 + rad,
        y1m = y1 - rad,
        lx0 = x0 + w * 0.2,
        lx1 = x1 - w * 0.2,
        lx2 = x0 + w * 0.55,
        ly1 = y0 + h * 0.68,
        ly2 = y0 + h * 0.5,
        ly3 = y0 + h * 0.32,
    )
}

/// Appearance stream for a free text box: text in Helvetica (PDFium falls
/// back to its stock Helvetica when the resource is missing), wrapped by
/// approximate glyph width, top-left aligned with 2pt padding.
fn free_text_appearance(r: Rect, text: &str, size: f32, c: Color) -> String {
    let pad = 2.0;
    let max_w = (r.w - pad * 2.0).max(1.0);
    let avg = size * 0.5; // Helvetica average advance
    let max_chars = ((max_w / avg).floor() as usize).max(1);
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if candidate.chars().count() > max_chars && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    };
    let mut out = format!(
        "q {} {} {} rg BT /Helv {size} Tf {lead} TL {x} {y} Td ",
        f(c.r),
        f(c.g),
        f(c.b),
        lead = size * 1.2,
        x = r.x + pad,
        y = r.y + r.h - pad - size,
    );
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("T* ");
        }
        out.push_str(&format!("({}) Tj ", esc(l)));
    }
    out.push_str("ET Q");
    out
}
