//! M6: content editing. Page objects (text, image, path), edit text runs,
//! move and delete objects, insert images, links, create PDFs from images,
//! export pages to images and text.
//!
//! Lives in its own file but extends `EngineState` (same thread, same PDFium
//! handles). Every mutation goes through `checkpoint` so undo/redo works.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::engine::{DocId, DocumentInfo, EngineState, Rect};
use crate::error::{Result, SheafError};

const FPDF_PAGEOBJ_TEXT: i32 = 1;
const FPDF_PAGEOBJ_PATH: i32 = 2;
const FPDF_PAGEOBJ_IMAGE: i32 = 3;
const FPDF_PAGEOBJ_SHADING: i32 = 4;
const FPDF_PAGEOBJ_FORM: i32 = 5;
#[allow(non_upper_case_globals)]
const FPDFBitmap_BGRA: i32 = 4;
const PDFACTION_URI: u32 = 3;
const PDFACTION_GOTO: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct PageObject {
    pub index: u32,
    /// "text" | "image" | "path" | "shading" | "form" | "unknown"
    pub kind: String,
    /// Bounding box in PDF points (origin bottom-left).
    pub rect: Rect,
    /// For text objects: the run's text, font name and size.
    pub text: Option<String>,
    pub font: Option<String>,
    pub font_size: Option<f32>,
    /// For images: pixel dimensions.
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkInfo {
    pub index: u32,
    pub rect: Rect,
    pub uri: Option<String>,
    pub page: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageSpec {
    pub path: String,
    /// Target rect in PDF points. Height 0 keeps the aspect ratio from width.
    pub rect: Rect,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextSpec {
    pub text: String,
    /// Baseline origin in PDF points.
    pub x: f32,
    pub y: f32,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_size")]
    pub font_size: f32,
    #[serde(default)]
    pub color: Option<crate::engine::Color>,
}
fn default_font() -> String {
    "Helvetica".into()
}
fn default_size() -> f32 {
    12.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct LinkSpec {
    pub rect: Rect,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub page: Option<u16>,
}

fn pdf(msg: impl Into<String>) -> SheafError {
    SheafError::Pdf(msg.into())
}

fn bounds(b: &dyn PdfiumLibraryBindings, o: FPDF_PAGEOBJECT) -> Rect {
    let (mut l, mut bo, mut r, mut t) = (0f32, 0f32, 0f32, 0f32);
    unsafe { b.FPDFPageObj_GetBounds(o, &mut l, &mut bo, &mut r, &mut t) };
    Rect { x: l, y: bo, w: r - l, h: t - bo }
}

impl EngineState {
    // ----- page objects -----

    pub(crate) fn list_page_objects(&self, id: DocId, index: u16) -> Result<Vec<PageObject>> {
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let tp = self.text_page(&p).ok();
        let n = unsafe { b.FPDFPage_CountObjects(p.page) }.max(0);
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let o = unsafe { b.FPDFPage_GetObject(p.page, i) };
            if o.is_null() {
                continue;
            }
            let ty = unsafe { b.FPDFPageObj_GetType(o) };
            let kind = match ty {
                FPDF_PAGEOBJ_TEXT => "text",
                FPDF_PAGEOBJ_PATH => "path",
                FPDF_PAGEOBJ_IMAGE => "image",
                FPDF_PAGEOBJ_SHADING => "shading",
                FPDF_PAGEOBJ_FORM => "form",
                _ => "unknown",
            };
            let mut po = PageObject {
                index: i as u32,
                kind: kind.into(),
                rect: bounds(b, o),
                text: None,
                font: None,
                font_size: None,
                image_width: None,
                image_height: None,
            };
            if ty == FPDF_PAGEOBJ_TEXT {
                if let Some(tp) = tp.as_ref() {
                    let len = unsafe {
                        b.FPDFTextObj_GetText(o, tp.tp, std::ptr::null_mut(), 0)
                    } as usize;
                    if len > 0 {
                        let mut buf = vec![0u16; len / 2 + 1];
                        unsafe {
                            b.FPDFTextObj_GetText(
                                o,
                                tp.tp,
                                buf.as_mut_ptr(),
                                len as std::os::raw::c_ulong,
                            )
                        };
                        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                        po.text = Some(String::from_utf16_lossy(&buf[..end]));
                    }
                }
                let mut size = 0f32;
                unsafe { b.FPDFTextObj_GetFontSize(o, &mut size) };
                po.font_size = Some(size);
                let font = unsafe { b.FPDFTextObj_GetFont(o) };
                if !font.is_null() {
                    let mut name = vec![0u8; 128];
                    let n = unsafe {
                        b.FPDFFont_GetBaseFontName(font, name.as_mut_ptr() as *mut _, name.len())
                    };
                    if n > 0 {
                        name.truncate((n as usize).saturating_sub(1).min(name.len()));
                        po.font = Some(String::from_utf8_lossy(&name).into_owned());
                    }
                }
            } else if ty == FPDF_PAGEOBJ_IMAGE {
                let (mut w, mut h) = (0u32, 0u32);
                unsafe { b.FPDFImageObj_GetImagePixelSize(o, &mut w, &mut h) };
                po.image_width = Some(w);
                po.image_height = Some(h);
            }
            out.push(po);
        }
        Ok(out)
    }

    fn with_object<T>(
        &self,
        id: DocId,
        index: u16,
        obj: u32,
        f: impl FnOnce(&dyn PdfiumLibraryBindings, FPDF_PAGE, FPDF_PAGEOBJECT) -> Result<T>,
    ) -> Result<T> {
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let o = unsafe { b.FPDFPage_GetObject(p.page, obj as i32) };
        if o.is_null() {
            return Err(pdf(format!("no page object {obj}")));
        }
        let r = f(b, p.page, o)?;
        unsafe { b.FPDFPage_GenerateContent(p.page) };
        Ok(r)
    }

    pub(crate) fn set_text_object(
        &mut self,
        id: DocId,
        index: u16,
        obj: u32,
        text: String,
        font_size: Option<f32>,
    ) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        self.with_object(id, index, obj, |b, _p, o| {
            if unsafe { b.FPDFPageObj_GetType(o) } != FPDF_PAGEOBJ_TEXT {
                return Err(pdf("not a text object"));
            }
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            wide.push(0);
            if unsafe { b.FPDFText_SetText(o, wide.as_ptr()) } == 0 {
                return Err(pdf("PDFium refused the new text (font may lack glyphs)"));
            }
            if let Some(s) = font_size {
                // No setter for font size in this PDFium build: scale the
                // object about its bottom-left corner instead.
                let mut cur = 0f32;
                unsafe { b.FPDFTextObj_GetFontSize(o, &mut cur) };
                if cur > 0.0 && (s - cur).abs() > 0.01 {
                    let k = (s / cur) as f64;
                    let r = bounds(b, o);
                    unsafe {
                        b.FPDFPageObj_Transform(o, 1.0, 0.0, 0.0, 1.0, -r.x as f64, -r.y as f64);
                        b.FPDFPageObj_Transform(o, k, 0.0, 0.0, k, 0.0, 0.0);
                        b.FPDFPageObj_Transform(o, 1.0, 0.0, 0.0, 1.0, r.x as f64, r.y as f64);
                    }
                }
            }
            Ok(())
        })?;
        self.info(id)
    }

    pub(crate) fn move_page_object(
        &mut self,
        id: DocId,
        index: u16,
        obj: u32,
        dx: f32,
        dy: f32,
        scale: f32,
    ) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        self.with_object(id, index, obj, |b, _p, o| {
            if scale != 1.0 && scale > 0.0 {
                // Scale about the object's own bottom-left corner.
                let r = bounds(b, o);
                unsafe {
                    b.FPDFPageObj_Transform(o, 1.0, 0.0, 0.0, 1.0, -r.x as f64, -r.y as f64);
                    b.FPDFPageObj_Transform(o, scale as f64, 0.0, 0.0, scale as f64, 0.0, 0.0);
                    b.FPDFPageObj_Transform(o, 1.0, 0.0, 0.0, 1.0, r.x as f64, r.y as f64);
                }
            }
            unsafe { b.FPDFPageObj_Transform(o, 1.0, 0.0, 0.0, 1.0, dx as f64, dy as f64) };
            Ok(())
        })?;
        self.info(id)
    }

    pub(crate) fn delete_page_object(&mut self, id: DocId, index: u16, obj: u32) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        self.with_object(id, index, obj, |b, p, o| {
            if unsafe { b.FPDFPage_RemoveObject(p, o) } == 0 {
                return Err(pdf("could not remove object"));
            }
            unsafe { b.FPDFPageObj_Destroy(o) };
            Ok(())
        })?;
        self.info(id)
    }

    pub(crate) fn add_text(&mut self, id: DocId, index: u16, spec: &TextSpec) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let font = unsafe { b.FPDFText_LoadStandardFont(d.handle, &spec.font) };
        if font.is_null() {
            return Err(pdf(format!("unknown standard font {}", spec.font)));
        }
        let obj = unsafe { b.FPDFPageObj_CreateTextObj(d.handle, font, spec.font_size.max(1.0)) };
        if obj.is_null() {
            return Err(pdf("could not create text object"));
        }
        let mut wide: Vec<u16> = spec.text.encode_utf16().collect();
        wide.push(0);
        unsafe {
            b.FPDFText_SetText(obj, wide.as_ptr());
            let c = spec.color.unwrap_or(crate::engine::Color { r: 0, g: 0, b: 0 });
            b.FPDFPageObj_SetFillColor(obj, c.r as u32, c.g as u32, c.b as u32, 255);
            b.FPDFPageObj_Transform(obj, 1.0, 0.0, 0.0, 1.0, spec.x as f64, spec.y as f64);
            b.FPDFPage_InsertObject(p.page, obj);
            b.FPDFPage_GenerateContent(p.page);
        }
        drop(p);
        self.info(id)
    }

    // ----- images -----

    fn load_image_bitmap(&self, path: &Path) -> Result<(FPDF_BITMAP, u32, u32)> {
        let img = image::open(path)
            .map_err(|e| pdf(format!("could not read image: {e}")))?
            .into_rgba8();
        let (w, h) = img.dimensions();
        let b = self.b.as_ref();
        let bmp = unsafe {
            b.FPDFBitmap_CreateEx(w as i32, h as i32, FPDFBitmap_BGRA, std::ptr::null_mut(), 0)
        };
        if bmp.is_null() {
            return Err(pdf("could not allocate bitmap"));
        }
        let stride = unsafe { b.FPDFBitmap_GetStride(bmp) } as usize;
        let buf = unsafe { b.FPDFBitmap_GetBuffer(bmp) } as *mut u8;
        for y in 0..h as usize {
            let row = unsafe { std::slice::from_raw_parts_mut(buf.add(y * stride), w as usize * 4) };
            for (x, px) in img.rows().nth(y).unwrap().enumerate() {
                let [r, g, bl, a] = px.0;
                // Premultiplied BGRA
                let pm = |c: u8| ((c as u32 * a as u32) / 255) as u8;
                row[x * 4] = pm(bl);
                row[x * 4 + 1] = pm(g);
                row[x * 4 + 2] = pm(r);
                row[x * 4 + 3] = a;
            }
        }
        Ok((bmp, w, h))
    }

    pub(crate) fn insert_image(&mut self, id: DocId, index: u16, spec: &ImageSpec) -> Result<DocumentInfo> {
        let (bmp, w, h) = self.load_image_bitmap(Path::new(&spec.path))?;
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let obj = unsafe { b.FPDFPageObj_NewImageObj(d.handle) };
        if obj.is_null() {
            unsafe { b.FPDFBitmap_Destroy(bmp) };
            return Err(pdf("could not create image object"));
        }
        let mut pages = [p.page];
        let ok = unsafe { b.FPDFImageObj_SetBitmap(pages.as_mut_ptr(), 1, obj, bmp) };
        unsafe { b.FPDFBitmap_Destroy(bmp) };
        if ok == 0 {
            unsafe { b.FPDFPageObj_Destroy(obj) };
            return Err(pdf("could not set image bitmap"));
        }
        let rw = spec.rect.w.max(1.0);
        let rh = if spec.rect.h > 0.0 {
            spec.rect.h
        } else {
            rw * h as f32 / w.max(1) as f32
        };
        let m = FS_MATRIX { a: rw, b: 0.0, c: 0.0, d: rh, e: spec.rect.x, f: spec.rect.y };
        unsafe {
            b.FPDFPageObj_SetMatrix(obj, &m);
            b.FPDFPage_InsertObject(p.page, obj);
            b.FPDFPage_GenerateContent(p.page);
        }
        drop(p);
        self.info(id)
    }

    /// Decoded pixels of an image object as PNG bytes.
    pub(crate) fn extract_image(&self, id: DocId, index: u16, obj: u32, path: &Path) -> Result<()> {
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let o = unsafe { b.FPDFPage_GetObject(p.page, obj as i32) };
        if o.is_null() || unsafe { b.FPDFPageObj_GetType(o) } != FPDF_PAGEOBJ_IMAGE {
            return Err(pdf("not an image object"));
        }
        // Raw decoded pixels first (source resolution); fall back to the
        // rendered form for images with unusual colour spaces or masks.
        let mut bmp = unsafe { b.FPDFImageObj_GetBitmap(o) };
        if bmp.is_null() {
            bmp = unsafe { b.FPDFImageObj_GetRenderedBitmap(d.handle, p.page, o) };
        }
        if bmp.is_null() {
            return Err(pdf("could not decode image"));
        }
        let r = self.bitmap_to_png(bmp);
        unsafe { b.FPDFBitmap_Destroy(bmp) };
        std::fs::write(path, r?)?;
        Ok(())
    }

    fn bitmap_to_png(&self, bmp: FPDF_BITMAP) -> Result<Vec<u8>> {
        let b = self.b.as_ref();
        let w = unsafe { b.FPDFBitmap_GetWidth(bmp) } as usize;
        let h = unsafe { b.FPDFBitmap_GetHeight(bmp) } as usize;
        let stride = unsafe { b.FPDFBitmap_GetStride(bmp) } as usize;
        let fmt = unsafe { b.FPDFBitmap_GetFormat(bmp) };
        let buf = unsafe { b.FPDFBitmap_GetBuffer(bmp) } as *const u8;
        let bpp = match fmt {
            1 => 1, // gray
            2 => 3, // BGR
            _ => 4, // BGRx / BGRA
        };
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            let row = unsafe { std::slice::from_raw_parts(buf.add(y * stride), w * bpp) };
            for x in 0..w {
                let dst = &mut rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                match bpp {
                    1 => {
                        dst[..3].fill(row[x]);
                        dst[3] = 255;
                    }
                    3 => {
                        dst[0] = row[x * 3 + 2];
                        dst[1] = row[x * 3 + 1];
                        dst[2] = row[x * 3];
                        dst[3] = 255;
                    }
                    _ => {
                        dst[0] = row[x * 4 + 2];
                        dst[1] = row[x * 4 + 1];
                        dst[2] = row[x * 4];
                        dst[3] = if fmt == 4 { row[x * 4 + 3] } else { 255 };
                    }
                }
            }
        }
        let mut png = Vec::new();
        image::write_buffer_with_format(
            &mut std::io::Cursor::new(&mut png),
            &rgba,
            w as u32,
            h as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|e| pdf(format!("png encode: {e}")))?;
        Ok(png)
    }

    // ----- links -----

    pub(crate) fn list_links(&self, id: DocId, index: u16) -> Result<Vec<LinkInfo>> {
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let mut out = Vec::new();
        let mut pos: i32 = 0;
        let mut link: FPDF_LINK = std::ptr::null_mut();
        let mut i = 0u32;
        while unsafe { b.FPDFLink_Enumerate(p.page, &mut pos, &mut link) } != 0 && !link.is_null() {
            let mut r = FS_RECTF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
            unsafe { b.FPDFLink_GetAnnotRect(link, &mut r) };
            let mut info = LinkInfo {
                index: i,
                rect: Rect { x: r.left, y: r.bottom, w: r.right - r.left, h: r.top - r.bottom },
                uri: None,
                page: None,
            };
            let action = unsafe { b.FPDFLink_GetAction(link) };
            if !action.is_null() {
                let ty = unsafe { b.FPDFAction_GetType(action) } as u32;
                if ty == PDFACTION_URI {
                    let len = unsafe {
                        b.FPDFAction_GetURIPath(d.handle, action, std::ptr::null_mut(), 0)
                    } as usize;
                    if len > 1 {
                        let mut buf = vec![0u8; len];
                        unsafe {
                            b.FPDFAction_GetURIPath(
                                d.handle,
                                action,
                                buf.as_mut_ptr() as *mut c_void,
                                len as std::os::raw::c_ulong,
                            )
                        };
                        buf.truncate(len - 1);
                        info.uri = Some(String::from_utf8_lossy(&buf).into_owned());
                    }
                } else if ty == PDFACTION_GOTO {
                    let dest = unsafe { b.FPDFAction_GetDest(d.handle, action) };
                    if !dest.is_null() {
                        let pi = unsafe { b.FPDFDest_GetDestPageIndex(d.handle, dest) };
                        if pi >= 0 {
                            info.page = Some(pi as u16);
                        }
                    }
                }
            } else {
                let dest = unsafe { b.FPDFLink_GetDest(d.handle, link) };
                if !dest.is_null() {
                    let pi = unsafe { b.FPDFDest_GetDestPageIndex(d.handle, dest) };
                    if pi >= 0 {
                        info.page = Some(pi as u16);
                    }
                }
            }
            out.push(info);
            i += 1;
        }
        Ok(out)
    }

    pub(crate) fn add_link(&mut self, id: DocId, index: u16, spec: &LinkSpec) -> Result<DocumentInfo> {
        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let a = unsafe { b.FPDFPage_CreateAnnot(p.page, 2 /* FPDF_ANNOT_LINK */) };
        if a.is_null() {
            return Err(pdf("could not create link annotation"));
        }
        let r = FS_RECTF {
            left: spec.rect.x,
            bottom: spec.rect.y,
            right: spec.rect.x + spec.rect.w,
            top: spec.rect.y + spec.rect.h,
        };
        unsafe { b.FPDFAnnot_SetRect(a, &r) };
        let ok = match (&spec.uri, spec.page) {
            (Some(u), _) if !u.trim().is_empty() => unsafe { b.FPDFAnnot_SetURI(a, u.trim()) },
            (_, Some(pg)) => {
                // PDFium has no public API for GoTo destinations on new
                // annotations; store the page as a URI fragment the viewer
                // understands (#page=N) so in-app navigation still works.
                unsafe { b.FPDFAnnot_SetURI(a, &format!("#page={}", pg + 1)) }
            }
            _ => 0,
        };
        unsafe { b.FPDFPage_CloseAnnot(a) };
        if ok == 0 {
            return Err(pdf("link needs a URL or a page"));
        }
        unsafe { b.FPDFPage_GenerateContent(p.page) };
        drop(p);
        self.info(id)
    }

    // ----- create and export -----

    /// New PDF with one page per image, sized to the image at 72 DPI (capped
    /// to fit within Letter/A4-ish 612x792 when larger, preserving aspect).
    pub(crate) fn create_from_images(&mut self, paths: &[PathBuf], out: &Path) -> Result<DocumentInfo> {
        let b = self.b.as_ref();
        let doc = unsafe { b.FPDF_CreateNewDocument() };
        if doc.is_null() {
            return Err(pdf("could not create document"));
        }
        for (i, path) in paths.iter().enumerate() {
            let (bmp, w, h) = match self.load_image_bitmap(path) {
                Ok(x) => x,
                Err(e) => {
                    unsafe { b.FPDF_CloseDocument(doc) };
                    return Err(e);
                }
            };
            let (mut pw, mut ph) = (w as f64, h as f64);
            let max = 792.0;
            if pw > max || ph > max {
                let s = (max / pw).min(max / ph);
                pw *= s;
                ph *= s;
            }
            let page = unsafe { b.FPDFPage_New(doc, i as i32, pw, ph) };
            let obj = unsafe { b.FPDFPageObj_NewImageObj(doc) };
            let mut pages = [page];
            unsafe {
                b.FPDFImageObj_SetBitmap(pages.as_mut_ptr(), 1, obj, bmp);
                b.FPDFBitmap_Destroy(bmp);
                let m = FS_MATRIX { a: pw as f32, b: 0.0, c: 0.0, d: ph as f32, e: 0.0, f: 0.0 };
                b.FPDFPageObj_SetMatrix(obj, &m);
                b.FPDFPage_InsertObject(page, obj);
                b.FPDFPage_GenerateContent(page);
                b.FPDF_ClosePage(page);
            }
        }
        let bytes = self.snapshot(doc, 2 /* FPDF_NO_INCREMENTAL */);
        unsafe { b.FPDF_CloseDocument(doc) };
        std::fs::write(out, bytes?)?;
        self.open(out.to_path_buf(), None)
    }

    /// Render pages to PNG files: `<dir>/<stem>-<n>.png`. Returns the paths.
    pub(crate) fn export_images(&self, id: DocId, pages: &[u16], dir: &Path, dpi: f32) -> Result<Vec<String>> {
        std::fs::create_dir_all(dir)?;
        let stem = self
            .doc(id)?
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "page".into());
        let mut out = Vec::new();
        for &pi in pages {
            let r = self.render(id, pi, dpi / 72.0, 0)?;
            use base64::Engine as _;
            let png = base64::engine::general_purpose::STANDARD
                .decode(&r.png_base64)
                .map_err(|e| pdf(format!("png: {e}")))?;
            let path = dir.join(format!("{stem}-{}.png", pi + 1));
            std::fs::write(&path, png)?;
            out.push(path.to_string_lossy().into_owned());
        }
        Ok(out)
    }

    /// Plain text of the selected pages, form-feed separated.
    pub(crate) fn export_text(&self, id: DocId, pages: &[u16]) -> Result<String> {
        let mut s = String::new();
        for (i, &pi) in pages.iter().enumerate() {
            if i > 0 {
                s.push('\u{c}');
                s.push('\n');
            }
            s.push_str(&self.page_text(id, pi)?.text);
            s.push('\n');
        }
        Ok(s)
    }
}
