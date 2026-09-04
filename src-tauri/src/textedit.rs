//! Paragraph-level text editing on top of PDFium's page-object API.
//!
//! PDF has no paragraphs: producers emit text as arbitrary runs ("Dumm",
//! "y "). This module rebuilds structure from geometry so the UI can offer
//! "click the paragraph, type, done":
//!
//! * runs on the same baseline (within a tolerance scaled by font size)
//!   form a **line**, ordered left to right;
//! * consecutive lines with similar leading and overlapping horizontal
//!   extent form a **block** (paragraph);
//! * committing a block deletes its runs and lays the new text out again,
//!   word wrapped to the block's original width using the real glyph
//!   widths of the block's dominant font, one text object per line.
//!
//! The font handle of the original run is reused for the new objects, so
//! embedded fonts keep working as long as the replacement text only needs
//! glyphs the font already has. When the font lacks a glyph PDFium reports
//! width 0 and we fall back to Helvetica for the whole block.
use std::collections::BTreeMap;

use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::engine::{Color, DocId, DocumentInfo, EngineState, Rect};
use crate::error::{Result, SheafError};

#[derive(Debug, Clone, Serialize)]
pub struct TextBlock {
    /// Stable within a page until the page content changes.
    pub id: u32,
    /// Union of the block's line boxes, PDF points, origin bottom-left.
    pub rect: Rect,
    /// Lines joined with '\n'. Soft wraps inside a paragraph are joined
    /// with a space so the editor shows flowing text.
    pub text: String,
    pub font: String,
    pub font_size: f32,
    /// Distance between consecutive baselines (== font_size for one line).
    pub leading: f32,
    pub color: Color,
    /// Page-object indices of every run in the block (for deletion).
    pub objects: Vec<u32>,
    /// Baseline y of the first line and left x of the block.
    pub baseline_x: f32,
    pub baseline_y: f32,
    pub line_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockEdit {
    pub id: u32,
    pub text: String,
    /// Wrap width in points; None keeps the block's current width.
    pub width: Option<f32>,
    /// Move the block by this many points (applied before layout).
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
    pub font_size: Option<f32>,
}

fn pdf(msg: impl Into<String>) -> SheafError {
    SheafError::Pdf(msg.into())
}

/// One text run with the geometry we need for grouping.
struct Run {
    index: u32,
    text: String,
    font: FPDF_FONT,
    font_name: String,
    size: f32,
    color: Color,
    /// Baseline origin (from the text matrix) and glyph bounds.
    ox: f32,
    oy: f32,
    rect: Rect,
}

impl EngineState {
    fn runs(&self, id: DocId, index: u16) -> Result<Vec<Run>> {
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;
        let tp = self.text_page(&p)?;
        let n = unsafe { b.FPDFPage_CountObjects(p.page) }.max(0);
        let mut out = Vec::new();
        for i in 0..n {
            let o = unsafe { b.FPDFPage_GetObject(p.page, i) };
            if o.is_null() || unsafe { b.FPDFPageObj_GetType(o) } != FPDF_PAGEOBJ_TEXT as i32 {
                continue;
            }
            let len = unsafe { b.FPDFTextObj_GetText(o, tp.tp, std::ptr::null_mut(), 0) } as usize;
            let mut text = String::new();
            if len > 0 {
                let mut buf = vec![0u16; len / 2 + 1];
                unsafe { b.FPDFTextObj_GetText(o, tp.tp, buf.as_mut_ptr(), len as std::os::raw::c_ulong) };
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                text = String::from_utf16_lossy(&buf[..end]);
            }
            if text.trim().is_empty() {
                continue;
            }
            let mut size = 0f32;
            unsafe { b.FPDFTextObj_GetFontSize(o, &mut size) };
            let font = unsafe { b.FPDFTextObj_GetFont(o) };
            let mut font_name = String::from("Helvetica");
            if !font.is_null() {
                let mut name = vec![0u8; 128];
                let n = unsafe { b.FPDFFont_GetBaseFontName(font, name.as_mut_ptr() as *mut _, name.len()) };
                if n > 1 {
                    name.truncate((n as usize - 1).min(name.len()));
                    font_name = String::from_utf8_lossy(&name).into_owned();
                }
            }
            let mut m = FS_MATRIX { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
            unsafe { b.FPDFPageObj_GetMatrix(o, &mut m) };
            // Effective size: matrix scale times nominal size (rotated text is
            // treated as upright; rare in body copy).
            let scale = (m.a * m.a + m.b * m.b).sqrt().max(0.01);
            let (mut r, mut g, mut bl, mut a) = (0u32, 0u32, 0u32, 0u32);
            unsafe { b.FPDFPageObj_GetFillColor(o, &mut r, &mut g, &mut bl, &mut a) };
            let (mut l, mut bo, mut ri, mut t) = (0f32, 0f32, 0f32, 0f32);
            unsafe { b.FPDFPageObj_GetBounds(o, &mut l, &mut bo, &mut ri, &mut t) };
            out.push(Run {
                index: i as u32,
                text,
                font,
                font_name,
                size: size * scale,
                color: Color { r: r as u8, g: g as u8, b: bl as u8 },
                ox: m.e,
                oy: m.f,
                rect: Rect { x: l, y: bo, w: ri - l, h: t - bo },
            });
        }
        Ok(out)
    }

    /// Group a page's text runs into paragraphs.
    pub(crate) fn text_blocks(&self, id: DocId, index: u16) -> Result<Vec<TextBlock>> {
        let mut runs = self.runs(id, index)?;
        // Lines: cluster by baseline. Sort by baseline descending (top first),
        // then x.
        runs.sort_by(|a, b| b.oy.partial_cmp(&a.oy).unwrap().then(a.ox.partial_cmp(&b.ox).unwrap()));
        let mut lines: Vec<Vec<Run>> = Vec::new();
        for r in runs {
            let tol = (r.size * 0.3).max(1.0);
            match lines.last_mut() {
                Some(line) if (line[0].oy - r.oy).abs() <= tol => line.push(r),
                _ => lines.push(vec![r]),
            }
        }
        for l in &mut lines {
            l.sort_by(|a, b| a.ox.partial_cmp(&b.ox).unwrap());
        }

        // Blocks: consecutive lines with compatible leading and overlapping x.
        struct L {
            runs: Vec<Run>,
            x0: f32,
            x1: f32,
            y: f32,
            size: f32,
        }
        let lines: Vec<L> = lines
            .into_iter()
            .map(|runs| {
                let x0 = runs.iter().map(|r| r.rect.x).fold(f32::MAX, f32::min);
                let x1 = runs.iter().map(|r| r.rect.x + r.rect.w).fold(f32::MIN, f32::max);
                let y = runs[0].oy;
                let size = runs.iter().map(|r| r.size).fold(0.0, f32::max);
                L { runs, x0, x1, y, size }
            })
            .collect();

        let mut blocks: Vec<Vec<L>> = Vec::new();
        for l in lines {
            let joined = match blocks.last() {
                Some(blk) => {
                    let prev = blk.last().unwrap();
                    let gap = prev.y - l.y;
                    let overlap = l.x0 < prev.x1 && prev.x0 < l.x1;
                    let same_size = (prev.size - l.size).abs() <= prev.size * 0.15;
                    let leading_ok = gap > 0.0 && gap <= l.size.max(prev.size) * 1.8;
                    let consistent = blk.len() < 2 || {
                        let g0 = blk[blk.len() - 2].y - prev.y;
                        (g0 - gap).abs() <= g0 * 0.25
                    };
                    overlap && same_size && leading_ok && consistent
                }
                None => false,
            };
            if joined {
                blocks.last_mut().unwrap().push(l);
            } else {
                blocks.push(vec![l]);
            }
        }

        let mut out = Vec::new();
        for (bi, blk) in blocks.iter().enumerate() {
            let first = &blk[0];
            let dom = first.runs.iter().max_by(|a, b| a.text.len().cmp(&b.text.len())).unwrap();
            let leading = if blk.len() > 1 { (blk[0].y - blk[1].y).abs() } else { dom.size * 1.2 };
            let text = blk
                .iter()
                .map(|l| {
                    let mut s = String::new();
                    for (i, r) in l.runs.iter().enumerate() {
                        // Insert a space when runs are visibly separated.
                        if i > 0 {
                            let prev = &l.runs[i - 1];
                            let gap = r.rect.x - (prev.rect.x + prev.rect.w);
                            if gap > r.size * 0.15 && !s.ends_with(' ') && !r.text.starts_with(' ') {
                                s.push(' ');
                            }
                        }
                        if s.ends_with(' ') && r.text.starts_with(' ') {
                            s.push_str(r.text.trim_start());
                        } else {
                            s.push_str(&r.text);
                        }
                    }
                    s.trim_end().to_string()
                })
                .collect::<Vec<_>>()
                .join(" ");
            let text = text.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
            let x0 = blk.iter().map(|l| l.x0).fold(f32::MAX, f32::min);
            let x1 = blk.iter().map(|l| l.x1).fold(f32::MIN, f32::max);
            let top = blk.iter().flat_map(|l| l.runs.iter()).map(|r| r.rect.y + r.rect.h).fold(f32::MIN, f32::max);
            let bottom = blk.iter().flat_map(|l| l.runs.iter()).map(|r| r.rect.y).fold(f32::MAX, f32::min);
            out.push(TextBlock {
                id: bi as u32,
                rect: Rect { x: x0, y: bottom, w: x1 - x0, h: top - bottom },
                text,
                font: dom.font_name.clone(),
                font_size: dom.size,
                leading,
                color: dom.color,
                objects: blk.iter().flat_map(|l| l.runs.iter().map(|r| r.index)).collect(),
                baseline_x: first.runs[0].ox.min(x0),
                baseline_y: first.y,
                line_count: blk.len() as u32,
            });
        }
        Ok(out)
    }

    /// Replace a block's text: delete its runs, lay the new text out again.
    pub(crate) fn set_text_block(&mut self, id: DocId, index: u16, edit: &BlockEdit) -> Result<DocumentInfo> {
        let blocks = self.text_blocks(id, index)?;
        let blk = blocks.iter().find(|b| b.id == edit.id).ok_or_else(|| pdf("text block not found"))?.clone();
        // Grab the font handle from the block's first run before we delete it.
        let runs = self.runs(id, index)?;
        let src = runs.iter().find(|r| r.index == blk.objects[0]).ok_or_else(|| pdf("block run vanished"))?;
        let font_handle = src.font;
        let color = src.color;
        let size = edit.font_size.unwrap_or(blk.font_size).max(1.0);
        let leading = if edit.font_size.is_some() { size * (blk.leading / blk.font_size).max(1.0) } else { blk.leading };
        let width = edit.width.unwrap_or(blk.rect.w).max(size);
        let x = blk.baseline_x + edit.dx;
        let y0 = blk.baseline_y + edit.dy;
        drop(runs);

        self.checkpoint(id)?;
        let d = self.doc(id)?;
        let b = self.b.as_ref();
        let p = self.page(d, index)?;

        // Prefer the original font; fall back to Helvetica when it cannot
        // measure (or lacks glyphs for) the new text.
        let mut font = font_handle;
        let mut fallback = false;
        if font.is_null() || !can_measure(b, font, size, &edit.text) {
            font = unsafe { b.FPDFText_LoadStandardFont(d.handle, "Helvetica") };
            fallback = true;
        }
        if font.is_null() {
            return Err(pdf("no usable font for the new text"));
        }
        let measure = |s: &str| -> f32 {
            s.chars().map(|c| glyph_w(b, font, size, c, fallback)).sum()
        };

        // Delete old runs, highest index first so indices stay valid.
        let mut idx = blk.objects.clone();
        idx.sort_unstable_by(|a, b| b.cmp(a));
        for i in idx {
            let o = unsafe { b.FPDFPage_GetObject(p.page, i as i32) };
            if !o.is_null() && unsafe { b.FPDFPage_RemoveObject(p.page, o) } != 0 {
                unsafe { b.FPDFPageObj_Destroy(o) };
            }
        }

        // Word wrap. Explicit newlines start a new line; empty line = blank.
        let mut lines: Vec<String> = Vec::new();
        for para in edit.text.split('\n') {
            let mut cur = String::new();
            for word in para.split(' ') {
                if word.is_empty() {
                    continue;
                }
                let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
                if cur.is_empty() || measure(&candidate) <= width + 0.01 {
                    cur = candidate;
                } else {
                    lines.push(std::mem::take(&mut cur));
                    cur = word.to_string();
                }
            }
            lines.push(cur);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }

        for (li, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let obj = unsafe { b.FPDFPageObj_CreateTextObj(d.handle, font, size) };
            if obj.is_null() {
                return Err(pdf("could not create text object"));
            }
            let mut wide: Vec<u16> = line.encode_utf16().collect();
            wide.push(0);
            unsafe {
                b.FPDFText_SetText(obj, wide.as_ptr());
                b.FPDFPageObj_SetFillColor(obj, color.r as u32, color.g as u32, color.b as u32, 255);
                b.FPDFPageObj_Transform(obj, 1.0, 0.0, 0.0, 1.0, x as f64, (y0 - leading * li as f32) as f64);
                b.FPDFPage_InsertObject(p.page, obj);
            }
        }
        unsafe { b.FPDFPage_GenerateContent(p.page) };
        drop(p);
        self.info(id)
    }
}

/// Width of one character in `font` at `size`. With `fallback` (standard
/// Helvetica) PDFium measures by unicode code point directly; for embedded
/// fonts the glyph index is font specific, so we go through the same call
/// and treat 0 as "not present".
fn glyph_w(b: &dyn PdfiumLibraryBindings, font: FPDF_FONT, size: f32, c: char, _fallback: bool) -> f32 {
    let mut w = 0f32;
    let ok = unsafe { b.FPDFFont_GetGlyphWidth(font, c as u32, size, &mut w) };
    if ok == 0 || w <= 0.0 {
        // Unmeasurable: assume an average glyph so wrapping stays sane.
        return size * 0.5;
    }
    w
}

/// True when the font yields a non-zero width for every non-space glyph.
fn can_measure(b: &dyn PdfiumLibraryBindings, font: FPDF_FONT, size: f32, text: &str) -> bool {
    let mut probe: BTreeMap<char, bool> = BTreeMap::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        let ok = *probe.entry(c).or_insert_with(|| {
            let mut w = 0f32;
            unsafe { b.FPDFFont_GetGlyphWidth(font, c as u32, size, &mut w) != 0 && w > 0.0 }
        });
        if !ok {
            return false;
        }
    }
    true
}
