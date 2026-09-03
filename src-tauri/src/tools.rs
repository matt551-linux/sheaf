//! M7: redact, compare, OCR, accessibility.
//!
//! * **Redact**: true content removal. Every page object intersecting a
//!   redaction box is handled: text runs are split so only the covered
//!   characters disappear (runs fully inside are removed), images and paths
//!   that overlap are removed entirely (an image partially under a box is
//!   rasterised with the box blacked out and reinserted), then an opaque
//!   black rectangle is drawn. Annotations under the box are deleted.
//! * **Compare**: word-level text diff between two documents (via `similar`)
//!   plus a per-page pixel difference image for visual comparison.
//! * **OCR**: `ocrs` (pure Rust) recognises text on rendered pages and writes
//!   it back as invisible text runs (render mode 3) so the page becomes
//!   searchable and selectable. Models are downloaded on first use.
//! * **Accessibility**: structural checks (tagged, language, title, alt text
//!   for images, text vs scanned pages) reported per document.

use std::path::Path;

use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::engine::{DocId, DocumentInfo, EngineState, Rect};
use crate::error::{Result, SheafError};

const FPDF_PAGEOBJ_TEXT: i32 = 1;
const FPDF_PAGEOBJ_IMAGE: i32 = 3;
const FPDF_FILLMODE_WINDING: i32 = 2;
const FPDF_TEXTRENDERMODE_INVISIBLE: i32 = 3;

fn pdf(msg: impl Into<String>) -> SheafError {
    SheafError::Pdf(msg.into())
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}
fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x - 0.5
        && inner.y >= outer.y - 0.5
        && inner.x + inner.w <= outer.x + outer.w + 0.5
        && inner.y + inner.h <= outer.y + outer.h + 0.5
}
fn obj_bounds(b: &dyn PdfiumLibraryBindings, o: FPDF_PAGEOBJECT) -> Rect {
    let (mut l, mut bo, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
    unsafe { b.FPDFPageObj_GetBounds(o, &mut l, &mut bo, &mut r, &mut t) };
    Rect { x: l, y: bo, w: r - l, h: t - bo }
}

// ---------- redact ----------

#[derive(Debug, Clone, Deserialize)]
pub struct RedactSpec {
    pub page: u16,
    pub rects: Vec<Rect>,
    /// Fill colour of the box (default black).
    #[serde(default)]
    pub color: Option<crate::engine::Color>,
    /// Also remove annotations that touch a box.
    #[serde(default = "t")]
    pub remove_annotations: bool,
}
fn t() -> bool {
    true
}

impl EngineState {
    pub(crate) fn redact(&mut self, id: DocId, spec: &RedactSpec) -> Result<DocumentInfo> {
        if spec.rects.is_empty() {
            return self.info(id);
        }
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, spec.page)?;
        let tp = self.text_page(&p)?;

        // Gather per-object decisions first (mutating while iterating shifts indices).
        let n = unsafe { b.FPDFPage_CountObjects(p.page) }.max(0);
        let mut to_remove: Vec<FPDF_PAGEOBJECT> = Vec::new();
        let mut to_reinsert: Vec<(String, f32, f32, Rect, Vec<(u32, u32, u32)>)> = Vec::new();
        let mut rasterize: Vec<(FPDF_PAGEOBJECT, Rect)> = Vec::new();
        for i in 0..n {
            let o = unsafe { b.FPDFPage_GetObject(p.page, i) };
            if o.is_null() {
                continue;
            }
            let ob = obj_bounds(b, o);
            let hits: Vec<&Rect> = spec.rects.iter().filter(|r| overlaps(r, &ob)).collect();
            if hits.is_empty() {
                continue;
            }
            let ty = unsafe { b.FPDFPageObj_GetType(o) };
            if ty == FPDF_PAGEOBJ_TEXT {
                if hits.iter().any(|r| contains(r, &ob)) {
                    to_remove.push(o);
                    continue;
                }
                // Partial: keep characters outside every box by rebuilding
                // the run from its surviving characters. PDFium cannot
                // remove single glyphs, so we replace the run with the kept
                // text and blank the covered glyphs with spaces, which keeps
                // the remaining glyphs in place.
                let len = unsafe { b.FPDFTextObj_GetText(o, tp.tp, std::ptr::null_mut(), 0) } as usize;
                let mut buf = vec![0u16; len / 2 + 1];
                unsafe {
                    b.FPDFTextObj_GetText(o, tp.tp, buf.as_mut_ptr(), len as std::os::raw::c_ulong)
                };
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let run = String::from_utf16_lossy(&buf[..end]);
                // Map each char to its box via the text page: find chars whose
                // centre lies inside this run's bounds.
                let chars = self.chars_of(tp.tp).1;
                let mut kept = String::new();
                let mut any_removed = false;
                let mut run_chars: Vec<&crate::engine::TextChar> = chars
                    .iter()
                    .filter(|c| {
                        let cx = c.x + c.w / 2.0;
                        let cy = c.y + c.h / 2.0;
                        cx >= ob.x - 0.5 && cx <= ob.x + ob.w + 0.5 && cy >= ob.y - 0.5 && cy <= ob.y + ob.h + 0.5
                    })
                    .collect();
                run_chars.sort_by(|a, c| a.x.partial_cmp(&c.x).unwrap_or(std::cmp::Ordering::Equal));
                if run_chars.len() == run.chars().count() {
                    for (tc, ch) in run_chars.iter().zip(run.chars()) {
                        let cr = Rect { x: tc.x, y: tc.y, w: tc.w, h: tc.h };
                        let cx = Rect { x: cr.x + cr.w * 0.25, y: cr.y + cr.h * 0.25, w: cr.w * 0.5, h: cr.h * 0.5 };
                        if hits.iter().any(|r| overlaps(r, &cx)) {
                            kept.push(' ');
                            any_removed = true;
                        } else {
                            kept.push(ch);
                        }
                    }
                } else {
                    // Could not align glyphs to characters: remove the whole run
                    // rather than risk leaking covered text.
                    to_remove.push(o);
                    continue;
                }
                if any_removed {
                    unsafe { b.FPDFText_SetText_str(o, &kept) };
                }
            } else if ty == FPDF_PAGEOBJ_IMAGE {
                if hits.iter().any(|r| contains(r, &ob)) {
                    to_remove.push(o);
                } else {
                    rasterize.push((o, ob));
                }
            } else {
                // Paths, shadings, forms: remove when overlapping. Vector art
                // cannot be partially cut safely; the black box covers the
                // rest of the visual, and the source data is gone.
                to_remove.push(o);
            }
        }
        drop(tp);

        // Images partially under a box: bake the box into the pixels.
        for (o, ob) in rasterize {
            let bmp = unsafe { b.FPDFImageObj_GetRenderedBitmap(d.handle, p.page, o) };
            if bmp.is_null() {
                to_remove.push(o);
                continue;
            }
            let w = unsafe { b.FPDFBitmap_GetWidth(bmp) } as f32;
            let h = unsafe { b.FPDFBitmap_GetHeight(bmp) } as f32;
            let c = spec.color.unwrap_or(crate::engine::Color { r: 0, g: 0, b: 0 });
            let argb = 0xFF000000u32 | ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32;
            for r in spec.rects.iter().filter(|r| overlaps(r, &ob)) {
                // PDF space -> bitmap space (y flipped).
                let x0 = ((r.x - ob.x) / ob.w * w).floor().max(0.0) as i32;
                let x1 = ((r.x + r.w - ob.x) / ob.w * w).ceil().min(w) as i32;
                let y0 = ((ob.y + ob.h - (r.y + r.h)) / ob.h * h).floor().max(0.0) as i32;
                let y1 = ((ob.y + ob.h - r.y) / ob.h * h).ceil().min(h) as i32;
                if x1 > x0 && y1 > y0 {
                    unsafe { b.FPDFBitmap_FillRect(bmp, x0, y0, x1 - x0, y1 - y0, argb as std::os::raw::c_ulong) };
                }
            }
            let mut m = FS_MATRIX { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
            unsafe { b.FPDFPageObj_GetMatrix(o, &mut m) };
            let img = unsafe { b.FPDFPageObj_NewImageObj(d.handle) };
            let mut pages = [p.page];
            unsafe {
                b.FPDFImageObj_SetBitmap(pages.as_mut_ptr(), 1, img, bmp);
                b.FPDFBitmap_Destroy(bmp);
                b.FPDFPageObj_SetMatrix(img, &m);
                b.FPDFPage_InsertObject(p.page, img);
            }
            to_remove.push(o);
            let _ = &mut to_reinsert;
        }

        for o in to_remove {
            unsafe {
                if b.FPDFPage_RemoveObject(p.page, o) != 0 {
                    b.FPDFPageObj_Destroy(o);
                }
            }
        }

        // Annotations touching a box.
        if spec.remove_annotations {
            let count = unsafe { b.FPDFPage_GetAnnotCount(p.page) };
            for i in (0..count).rev() {
                let a = unsafe { b.FPDFPage_GetAnnot(p.page, i) };
                if a.is_null() {
                    continue;
                }
                let mut fr = FS_RECTF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
                unsafe { b.FPDFAnnot_GetRect(a, &mut fr) };
                unsafe { b.FPDFPage_CloseAnnot(a) };
                let ar = Rect { x: fr.left.min(fr.right), y: fr.bottom.min(fr.top), w: (fr.right - fr.left).abs(), h: (fr.top - fr.bottom).abs() };
                if spec.rects.iter().any(|r| overlaps(r, &ar)) {
                    unsafe { b.FPDFPage_RemoveAnnot(p.page, i) };
                }
            }
        }

        // Opaque boxes on top.
        let c = spec.color.unwrap_or(crate::engine::Color { r: 0, g: 0, b: 0 });
        for r in &spec.rects {
            let path = unsafe { b.FPDFPageObj_CreateNewRect(r.x, r.y, r.w, r.h) };
            unsafe {
                b.FPDFPageObj_SetFillColor(path, c.r as u32, c.g as u32, c.b as u32, 255);
                b.FPDFPath_SetDrawMode(path, FPDF_FILLMODE_WINDING, 0);
                b.FPDFPage_InsertObject(p.page, path);
            }
        }
        unsafe { b.FPDFPage_GenerateContent(p.page) };
        drop(p);
        self.info(id)
    }

    /// Rects of every occurrence of `query` on the given pages, for "redact
    /// by search". Reuses the search machinery (one rect per line hit).
    pub(crate) fn redact_search_rects(&self, id: DocId, query: &str, case: bool, whole: bool) -> Result<Vec<(u16, Rect)>> {
        let hits = self.search(id, query, case, whole)?;
        Ok(hits
            .into_iter()
            .flat_map(|h| h.rects.into_iter().map(move |r| (h.page_index, r)))
            .collect())
    }

    // ---------- compare ----------

    pub(crate) fn compare_text(&self, a: DocId, b: DocId) -> Result<CompareResult> {
        use similar::{ChangeTag, TextDiff};
        let pa = self.info(a)?.page_count;
        let pb = self.info(b)?.page_count;
        let mut pages = Vec::new();
        let mut total_ins = 0;
        let mut total_del = 0;
        for i in 0..pa.max(pb) {
            let ta = if i < pa { self.page_text(a, i)?.text } else { String::new() };
            let tb = if i < pb { self.page_text(b, i)?.text } else { String::new() };
            let diff = TextDiff::from_words(&ta, &tb);
            let mut segs = Vec::new();
            let (mut ins, mut del) = (0, 0);
            for ch in diff.iter_all_changes() {
                let kind = match ch.tag() {
                    ChangeTag::Equal => "equal",
                    ChangeTag::Insert => {
                        ins += 1;
                        "insert"
                    }
                    ChangeTag::Delete => {
                        del += 1;
                        "delete"
                    }
                };
                let text = ch.value().to_string();
                if let Some(last) = segs.last_mut() {
                    let last: &mut DiffSegment = last;
                    if last.kind == kind {
                        last.text.push_str(&text);
                        continue;
                    }
                }
                segs.push(DiffSegment { kind: kind.into(), text });
            }
            total_ins += ins;
            total_del += del;
            pages.push(PageDiff { page: i, inserted: ins, deleted: del, segments: segs });
        }
        Ok(CompareResult { pages, inserted: total_ins, deleted: total_del })
    }

    /// Visual diff of one page: renders both and returns a PNG where
    /// unchanged pixels are faded and differences are painted red/green
    /// (only in A = red, only in B = green).
    pub(crate) fn compare_visual(&self, a: DocId, b: DocId, page: u16, scale: f32) -> Result<crate::engine::RenderedPage> {
        use base64::Engine as _;
        let ra = self.render(a, page, scale, 0)?;
        let rb = self.render(b, page, scale, 0)?;
        let dec = |s: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(s)
                .ok()
                .and_then(|v| image::load_from_memory(&v).ok())
                .map(|i| i.into_rgba8())
        };
        let (ia, ib) = (dec(&ra.png_base64).ok_or_else(|| pdf("decode"))?, dec(&rb.png_base64).ok_or_else(|| pdf("decode"))?);
        let w = ia.width().max(ib.width());
        let h = ia.height().max(ib.height());
        let mut out = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
        let get = |img: &image::RgbaImage, x: u32, y: u32| -> u8 {
            if x < img.width() && y < img.height() {
                let p = img.get_pixel(x, y).0;
                ((p[0] as u32 + p[1] as u32 + p[2] as u32) / 3) as u8
            } else {
                255
            }
        };
        let mut changed = 0u64;
        for y in 0..h {
            for x in 0..w {
                let ga = get(&ia, x, y);
                let gb = get(&ib, x, y);
                let px = if (ga as i32 - gb as i32).abs() > 40 {
                    changed += 1;
                    if ga < gb {
                        image::Rgba([220, 30, 30, 255]) // only in A
                    } else {
                        image::Rgba([30, 160, 60, 255]) // only in B
                    }
                } else {
                    let f = 180 + (ga as u32 * 75 / 255) as u8; // faded grey
                    image::Rgba([f, f, f, 255])
                };
                out.put_pixel(x, y, px);
            }
        }
        let mut png = Vec::new();
        image::write_buffer_with_format(&mut std::io::Cursor::new(&mut png), &out, w, h, image::ColorType::Rgba8, image::ImageFormat::Png)
            .map_err(|e| pdf(format!("png: {e}")))?;
        let _ = changed;
        Ok(crate::engine::RenderedPage {
            index: page,
            width_px: w,
            height_px: h,
            png_base64: base64::engine::general_purpose::STANDARD.encode(png),
        })
    }

    // ---------- OCR ----------

    /// Recognise text on `pages` and add it as invisible text runs. Returns
    /// the number of lines added. `models_dir` holds the two .rten files.
    pub(crate) fn ocr_pages(&mut self, id: DocId, pages: &[u16], models_dir: &Path, dpi: f32) -> Result<OcrResult> {
        let engine = ocr_engine(models_dir)?;
        let mut lines_total = 0u32;
        let mut text_all = String::new();
        let scale = dpi / 72.0;
        // Render first (immutable), then mutate.
        let mut per_page: Vec<(u16, Vec<(String, Rect)>)> = Vec::new();
        for &pi in pages {
            use base64::Engine as _;
            let r = self.render(id, pi, scale, 0)?;
            let png = base64::engine::general_purpose::STANDARD.decode(&r.png_base64).map_err(|e| pdf(format!("{e}")))?;
            let img = image::load_from_memory(&png).map_err(|e| pdf(format!("{e}")))?.into_rgb8();
            let src = ocrs::ImageSource::from_bytes(img.as_raw(), img.dimensions()).map_err(|e| pdf(format!("ocr input: {e}")))?;
            let input = engine.prepare_input(src).map_err(|e| pdf(format!("ocr: {e}")))?;
            let words = engine.detect_words(&input).map_err(|e| pdf(format!("ocr detect: {e}")))?;
            let line_rects = engine.find_text_lines(&input, &words);
            let lines = engine.recognize_text(&input, &line_rects).map_err(|e| pdf(format!("ocr recognise: {e}")))?;
            let ph = r.height_px as f32;
            let mut found = Vec::new();
            for l in lines.into_iter().flatten() {
                let s = l.to_string();
                if s.trim().len() < 2 {
                    continue;
                }
                use ocrs::TextItem;
                let br = l.bounding_rect();
                let (left, top, right, bottom) = (br.left() as f32, br.top() as f32, br.right() as f32, br.bottom() as f32);
                // pixels (top-left origin) -> PDF points (bottom-left origin)
                let rect = Rect {
                    x: left / scale,
                    y: (ph - bottom) / scale,
                    w: (right - left) / scale,
                    h: (bottom - top) / scale,
                };
                text_all.push_str(&s);
                text_all.push('\n');
                found.push((s, rect));
            }
            per_page.push((pi, found));
        }
        if per_page.iter().all(|(_, f)| f.is_empty()) {
            return Ok(OcrResult { lines: 0, text: text_all });
        }
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let font = unsafe { b.FPDFText_LoadStandardFont(d.handle, "Helvetica") };
        for (pi, found) in per_page {
            let p = self.page(d, pi)?;
            for (s, rect) in found {
                let size = (rect.h * 0.85).clamp(4.0, 72.0);
                let obj = unsafe { b.FPDFPageObj_CreateTextObj(d.handle, font, size) };
                if obj.is_null() {
                    continue;
                }
                unsafe {
                    b.FPDFText_SetText_str(obj, &s);
                    b.FPDFTextObj_SetTextRenderMode(obj, FPDF_TEXTRENDERMODE_INVISIBLE);
                    // Stretch horizontally so selection spans the printed line.
                    let natural = obj_bounds(b, obj).w.max(1.0);
                    let sx = (rect.w / natural) as f64;
                    b.FPDFPageObj_Transform(obj, sx, 0.0, 0.0, 1.0, rect.x as f64, (rect.y + rect.h * 0.2) as f64);
                    b.FPDFPage_InsertObject(p.page, obj);
                }
                lines_total += 1;
            }
            unsafe { b.FPDFPage_GenerateContent(p.page) };
        }
        Ok(OcrResult { lines: lines_total, text: text_all })
    }

    // ---------- accessibility ----------

    pub(crate) fn accessibility_report(&self, id: DocId) -> Result<AccessibilityReport> {
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let info = self.info(id)?;
        let mut issues = Vec::new();
        let mut checks = Vec::new();
        let mut push = |ok: bool, name: &str, detail: String| {
            checks.push(AccessibilityCheck { name: name.into(), ok, detail: detail.clone() });
            if !ok {
                issues.push(detail);
            }
        };

        // Title
        push(info.title.as_deref().map(|t| !t.trim().is_empty()).unwrap_or(false), "Document title", if info.title.is_some() { "Title is set.".into() } else { "No document title (File > Properties).".into() });

        // Tagged structure: probe the catalog via lopdf (cheap, no PDFium API).
        let (tagged, lang) = catalog_flags(&d.bytes, d.password.as_deref());
        push(tagged, "Tagged PDF", if tagged { "Document has a structure tree (/MarkInfo /Marked true).".into() } else { "Not tagged: screen readers get no reading order or headings. Sheaf cannot add tags yet; export from the source application with tagging on.".into() });
        push(lang.is_some(), "Language", lang.clone().map(|l| format!("Language is {l}.")).unwrap_or_else(|| "No /Lang set on the document.".into()));

        // Per page: text present, images with no alt text (approximation:
        // untagged docs have no alt text at all).
        let mut scanned_pages = Vec::new();
        let mut image_count = 0u32;
        for i in 0..info.page_count {
            let p = self.page(d, i)?;
            let n = unsafe { b.FPDFPage_CountObjects(p.page) }.max(0);
            let mut has_text = false;
            for k in 0..n {
                let o = unsafe { b.FPDFPage_GetObject(p.page, k) };
                match unsafe { b.FPDFPageObj_GetType(o) } {
                    FPDF_PAGEOBJ_TEXT => has_text = true,
                    FPDF_PAGEOBJ_IMAGE => image_count += 1,
                    _ => {}
                }
            }
            if !has_text && image_count > 0 {
                scanned_pages.push(i + 1);
            }
        }
        push(scanned_pages.is_empty(), "Text on every page", if scanned_pages.is_empty() { "Every page has selectable text.".into() } else { format!("Pages without text (likely scans): {:?}. Run OCR to add a text layer.", scanned_pages) });
        push(image_count == 0 || tagged, "Image alternative text", if image_count == 0 { "No images.".into() } else if tagged { format!("{image_count} image(s); check alt text in the structure tree.") } else { format!("{image_count} image(s) with no alternative text (document is untagged).") });

        let outline = self.outline(id).map(|o| !o.is_empty()).unwrap_or(false);
        push(outline || info.page_count < 10, "Bookmarks", if outline { "Document has bookmarks.".into() } else if info.page_count < 10 { "Short document; bookmarks optional.".into() } else { "Long document without bookmarks.".into() });

        Ok(AccessibilityReport { checks, issues })
    }
}

fn catalog_flags(bytes: &[u8], password: Option<&str>) -> (bool, Option<String>) {
    let doc = match lopdf::Document::load_mem(bytes) {
        Ok(d) if !d.is_encrypted() => d,
        _ => match lopdf::Document::load_mem_with_options(bytes, lopdf::LoadOptions::with_password(password.unwrap_or(""))) {
            Ok(d) => d,
            Err(_) => return (false, None),
        },
    };
    let Ok(cat) = doc.catalog() else { return (false, None) };
    let tagged = cat
        .get(b"MarkInfo")
        .ok()
        .and_then(|m| match m {
            lopdf::Object::Dictionary(d) => Some(d.clone()),
            lopdf::Object::Reference(r) => doc.get_dictionary(*r).ok().cloned(),
            _ => None,
        })
        .and_then(|m| m.get(b"Marked").ok().and_then(|o| o.as_bool().ok()))
        .unwrap_or(false)
        && cat.has(b"StructTreeRoot");
    let lang = cat
        .get(b"Lang")
        .ok()
        .and_then(|o| o.as_str().ok())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .filter(|s| !s.is_empty());
    (tagged, lang)
}

// ---------- OCR engine cache ----------

const OCR_DETECT_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
const OCR_RECOG_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// True when both model files are present.
pub fn ocr_models_present(dir: &Path) -> bool {
    dir.join("text-detection.rten").exists() && dir.join("text-recognition.rten").exists()
}

/// Download the two models (about 20 MB) into `dir`.
#[cfg(feature = "ocr-download")]
pub fn download_ocr_models(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for (url, name) in [(OCR_DETECT_URL, "text-detection.rten"), (OCR_RECOG_URL, "text-recognition.rten")] {
        let target = dir.join(name);
        if target.exists() {
            continue;
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .build(),
            )
            .build()
            .into();
        let resp = agent
            .get(url)
            .call()
            .map_err(|e| pdf(format!("download {name}: {e}")))?;
        let bytes = resp
            .into_body()
            .with_config()
            .limit(200 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| pdf(format!("download {name}: {e}")))?;
        let tmp = target.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &target)?;
    }
    Ok(())
}

#[cfg(not(feature = "ocr-download"))]
pub fn download_ocr_models(_dir: &Path) -> Result<()> {
    Err(SheafError::Engine("built without OCR model download".into()))
}

fn ocr_engine(dir: &Path) -> Result<ocrs::OcrEngine> {
    if !ocr_models_present(dir) {
        return Err(SheafError::Engine("OCR models are not downloaded yet".into()));
    }
    let det = rten::Model::load_file(dir.join("text-detection.rten")).map_err(|e| pdf(format!("ocr model: {e}")))?;
    let rec = rten::Model::load_file(dir.join("text-recognition.rten")).map_err(|e| pdf(format!("ocr model: {e}")))?;
    ocrs::OcrEngine::new(ocrs::OcrEngineParams { detection_model: Some(det), recognition_model: Some(rec), ..Default::default() })
        .map_err(|e| pdf(format!("ocr engine: {e}")))
}

// ---------- result types ----------

#[derive(Debug, Clone, Serialize)]
pub struct DiffSegment {
    pub kind: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct PageDiff {
    pub page: u16,
    pub inserted: u32,
    pub deleted: u32,
    pub segments: Vec<DiffSegment>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CompareResult {
    pub pages: Vec<PageDiff>,
    pub inserted: u32,
    pub deleted: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct OcrResult {
    pub lines: u32,
    pub text: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccessibilityCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccessibilityReport {
    pub checks: Vec<AccessibilityCheck>,
    pub issues: Vec<String>,
}
