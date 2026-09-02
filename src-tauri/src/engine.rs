//! The PDF engine runs on a single dedicated thread that owns the PDFium
//! bindings and every open document. Tauri commands talk to it through a
//! channel. This keeps all PDFium access serialized (PDFium is not thread
//! safe) and gives the Tauri state a `'static` handle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;

use base64::Engine as _;
use pdfium_render::prelude::*;
use serde::Serialize;

use crate::error::{Result, SheafError};

pub type DocId = u32;

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub index: u16,
    /// Width in PDF points (1/72 inch), after page rotation.
    pub width: f32,
    pub height: f32,
    pub rotation: u16,
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
    pub creator: Option<String>,
    pub producer: Option<String>,
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
    /// Bounds in PDF points, origin bottom-left (PDF space).
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
pub struct SearchHit {
    pub page_index: u16,
    pub start: usize,
    pub len: usize,
    pub context: String,
}

enum Request {
    Open {
        path: PathBuf,
        password: Option<String>,
        reply: Sender<Result<DocumentInfo>>,
    },
    Close {
        id: DocId,
        reply: Sender<Result<()>>,
    },
    Render {
        id: DocId,
        page: u16,
        scale: f32,
        rotation: u16,
        reply: Sender<Result<RenderedPage>>,
    },
    Text {
        id: DocId,
        page: u16,
        reply: Sender<Result<PageText>>,
    },
    Outline {
        id: DocId,
        reply: Sender<Result<Vec<OutlineNode>>>,
    },
    Search {
        id: DocId,
        query: String,
        case_sensitive: bool,
        reply: Sender<Result<Vec<SearchHit>>>,
    },
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
                let pdfium = match bind_pdfium(library_dir.as_deref()) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // The engine thread lives for the whole process; leaking the
                // bindings lets documents borrow them for `'static`.
                let pdfium: &'static Pdfium = Box::leak(Box::new(pdfium));
                let _ = ready_tx.send(Ok(()));
                let mut state = EngineState {
                    pdfium,
                    docs: HashMap::new(),
                    paths: HashMap::new(),
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
        self.call(|reply| Request::Open {
            path,
            password,
            reply,
        })
    }
    pub fn close(&self, id: DocId) -> Result<()> {
        self.call(|reply| Request::Close { id, reply })
    }
    pub fn render(&self, id: DocId, page: u16, scale: f32, rotation: u16) -> Result<RenderedPage> {
        self.call(|reply| Request::Render {
            id,
            page,
            scale,
            rotation,
            reply,
        })
    }
    pub fn text(&self, id: DocId, page: u16) -> Result<PageText> {
        self.call(|reply| Request::Text { id, page, reply })
    }
    pub fn outline(&self, id: DocId) -> Result<Vec<OutlineNode>> {
        self.call(|reply| Request::Outline { id, reply })
    }
    pub fn search(&self, id: DocId, query: String, case_sensitive: bool) -> Result<Vec<SearchHit>> {
        self.call(|reply| Request::Search {
            id,
            query,
            case_sensitive,
            reply,
        })
    }
}

fn bind_pdfium(library_dir: Option<&Path>) -> Result<Pdfium> {
    let bindings = match library_dir {
        Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir))
            .or_else(|_| Pdfium::bind_to_system_library()),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|e| SheafError::Engine(format!("could not load PDFium: {e:?}")))?;
    Ok(Pdfium::new(bindings))
}

struct EngineState {
    pdfium: &'static Pdfium,
    docs: HashMap<DocId, PdfDocument<'static>>,
    paths: HashMap<DocId, PathBuf>,
    next_id: DocId,
}

impl EngineState {
    fn handle(&mut self, req: Request) {
        match req {
            Request::Open {
                path,
                password,
                reply,
            } => {
                let _ = reply.send(self.open(path, password));
            }
            Request::Close { id, reply } => {
                self.docs.remove(&id);
                self.paths.remove(&id);
                let _ = reply.send(Ok(()));
            }
            Request::Render {
                id,
                page,
                scale,
                rotation,
                reply,
            } => {
                let _ = reply.send(
                    self.doc(id)
                        .and_then(|d| render_page(d, page, scale, rotation)),
                );
            }
            Request::Text { id, page, reply } => {
                let _ = reply.send(self.doc(id).and_then(|d| page_text(d, page)));
            }
            Request::Outline { id, reply } => {
                let _ = reply.send(self.doc(id).map(outline));
            }
            Request::Search {
                id,
                query,
                case_sensitive,
                reply,
            } => {
                let _ = reply.send(self.doc(id).and_then(|d| search(d, &query, case_sensitive)));
            }
        }
    }

    fn doc(&self, id: DocId) -> Result<&PdfDocument<'static>> {
        self.docs.get(&id).ok_or(SheafError::NoSuchDocument(id))
    }

    fn open(&mut self, path: PathBuf, password: Option<String>) -> Result<DocumentInfo> {
        let doc = self
            .pdfium
            .load_pdf_from_file(&path, password.as_deref())
            .map_err(|e| match e {
                PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                    SheafError::PasswordRequired
                }
                other => SheafError::Pdf(format!("{other:?}")),
            })?;

        let id = self.next_id;
        self.next_id += 1;

        let pages: Vec<PageInfo> = doc
            .pages()
            .iter()
            .enumerate()
            .map(|(i, p)| PageInfo {
                index: i as u16,
                width: p.width().value,
                height: p.height().value,
                rotation: match p.rotation().unwrap_or(PdfPageRenderRotation::None) {
                    PdfPageRenderRotation::None => 0,
                    PdfPageRenderRotation::Degrees90 => 90,
                    PdfPageRenderRotation::Degrees180 => 180,
                    PdfPageRenderRotation::Degrees270 => 270,
                },
            })
            .collect();

        let meta = |tag: PdfDocumentMetadataTagType| {
            doc.metadata()
                .get(tag)
                .map(|t| t.value().to_string())
                .filter(|s| !s.is_empty())
        };

        let info = DocumentInfo {
            id,
            path: path.to_string_lossy().into_owned(),
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            page_count: pages.len() as u16,
            pages,
            title: meta(PdfDocumentMetadataTagType::Title),
            author: meta(PdfDocumentMetadataTagType::Author),
            subject: meta(PdfDocumentMetadataTagType::Subject),
            creator: meta(PdfDocumentMetadataTagType::Creator),
            producer: meta(PdfDocumentMetadataTagType::Producer),
        };

        self.docs.insert(id, doc);
        self.paths.insert(id, path);
        Ok(info)
    }
}

fn get_page<'a>(doc: &'a PdfDocument<'static>, index: u16) -> Result<PdfPage<'a>> {
    doc.pages()
        .get(index.into())
        .map_err(|_| SheafError::NoSuchPage(index))
}

fn render_page(
    doc: &PdfDocument<'static>,
    index: u16,
    scale: f32,
    rotation: u16,
) -> Result<RenderedPage> {
    let page = get_page(doc, index)?;
    let scale = scale.clamp(0.05, 16.0);
    let rot = match rotation % 360 {
        90 => PdfPageRenderRotation::Degrees90,
        180 => PdfPageRenderRotation::Degrees180,
        270 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    };
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(scale)
        .rotate(rot, true)
        .render_form_data(true)
        .render_annotations(true)
        .use_lcd_text_rendering(false);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| SheafError::Pdf(format!("render failed: {e:?}")))?;
    let img = bitmap
        .as_image()
        .map_err(|e| SheafError::Pdf(format!("bitmap conversion failed: {e:?}")))?
        .into_rgba8();
    let (w, h) = (img.width(), img.height());
    let mut png = Vec::new();
    image::write_buffer_with_format(
        &mut std::io::Cursor::new(&mut png),
        &img,
        w,
        h,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| SheafError::Pdf(format!("png encode failed: {e}")))?;
    Ok(RenderedPage {
        index,
        width_px: w,
        height_px: h,
        png_base64: base64::engine::general_purpose::STANDARD.encode(png),
    })
}

fn page_text(doc: &PdfDocument<'static>, index: u16) -> Result<PageText> {
    let page = get_page(doc, index)?;
    let text = page
        .text()
        .map_err(|e| SheafError::Pdf(format!("text extraction failed: {e:?}")))?;
    let chars = text
        .chars()
        .iter()
        .map(|c| {
            let b = c.loose_bounds().unwrap_or(PdfRect::ZERO);
            TextChar {
                ch: c
                    .unicode_char()
                    .map(|ch| ch.to_string())
                    .unwrap_or_default(),
                x: b.left().value,
                y: b.bottom().value,
                w: b.width().value,
                h: b.height().value,
            }
        })
        .collect();
    Ok(PageText {
        index,
        text: text.all(),
        chars,
    })
}

fn outline(doc: &PdfDocument<'static>) -> Vec<OutlineNode> {
    fn node(b: &PdfBookmark<'_>) -> OutlineNode {
        OutlineNode {
            title: b.title().unwrap_or_default(),
            page_index: b
                .destination()
                .and_then(|d| d.page_index().ok())
                .map(|i| i as u16),
            children: b.iter_direct_children().map(|c| node(&c)).collect(),
        }
    }
    let bookmarks = doc.bookmarks();
    match bookmarks.root() {
        Some(root) => std::iter::once(root.clone())
            .chain(root.iter_siblings())
            .map(|b| node(&b))
            .collect(),
        None => Vec::new(),
    }
}

fn search(doc: &PdfDocument<'static>, query: &str, case_sensitive: bool) -> Result<Vec<SearchHit>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut hits = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let Ok(text) = page.text() else { continue };
        let all = text.all();
        let hay = if case_sensitive {
            all.clone()
        } else {
            all.to_lowercase()
        };
        let mut from = 0;
        while let Some(pos) = hay[from..].find(&needle) {
            let start = from + pos;
            let ctx_start = all[..start]
                .char_indices()
                .rev()
                .nth(30)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let ctx_end = (start + needle.len() + 30).min(all.len());
            let ctx_end = all
                .char_indices()
                .map(|(i, _)| i)
                .filter(|&i| i >= ctx_end)
                .next()
                .unwrap_or(all.len());
            hits.push(SearchHit {
                page_index: i as u16,
                start,
                len: needle.len(),
                context: all[ctx_start..ctx_end].replace('\n', " "),
            });
            from = start + needle.len().max(1);
            if from >= hay.len() {
                break;
            }
        }
    }
    Ok(hits)
}
