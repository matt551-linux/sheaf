//! Integration tests against the PDF engine using real fixture PDFs.
//! Requires the PDFium binary fetched by `node scripts/fetch-pdfium.mjs`.

use std::path::PathBuf;

use sheaf_lib::engine::{
    AnnotKind, AnnotationPatch, AnnotationSpec, Color, Engine, Rect, SaveOptions,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("fixtures")
}

fn engine() -> Engine {
    Engine::start(sheaf_lib::pdfium_dir()).expect("engine starts")
}

/// Decode a base64 PNG and sample the pixel at (x, y).
fn pixel(png_b64: &str, x: u32, y: u32) -> [u8; 4] {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_b64)
        .unwrap();
    let img = image::load_from_memory(&bytes).unwrap().into_rgba8();
    img.get_pixel(x, y).0
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("sheaf-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn spec_annot(kind: AnnotKind, rect: Rect) -> AnnotationSpec {
    spec(kind, rect)
}

fn spec(kind: AnnotKind, rect: Rect) -> AnnotationSpec {
    AnnotationSpec {
        kind,
        rect,
        contents: String::new(),
        author: "Test".into(),
        color: Some(Color { r: 255, g: 0, b: 0 }),
        interior_color: None,
        border_width: 2.0,
        quads: vec![],
        ink: vec![],
        font_size: 12.0,
    }
}

#[test]
fn opens_and_reports_pages() {
    let e = engine();
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    assert_eq!(info.page_count, 1);
    assert_eq!(info.file_name, "sample.pdf");
    let p = &info.pages[0];
    assert!((p.width - 595.0).abs() < 1.0, "A4 width, got {}", p.width);
    assert!(
        (p.height - 842.0).abs() < 1.0,
        "A4 height, got {}",
        p.height
    );
    assert!(info.file_size > 1000);
    assert!(!info.encrypted);
    assert!(!info.modified);
    e.close(info.id).unwrap();
    assert!(e.render(info.id, 0, 1.0, 0).is_err(), "closed doc is gone");
}

#[test]
fn renders_page_to_png() {
    let e = engine();
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    let r = e.render(info.id, 0, 1.0, 0).unwrap();
    assert_eq!(r.width_px, 595);
    assert_eq!(r.height_px, 842);
    assert!(r.png_base64.len() > 1000);
    let r2 = e.render(info.id, 0, 2.0, 0).unwrap();
    assert_eq!(r2.width_px, 1190);
    let r3 = e.render(info.id, 0, 1.0, 90).unwrap();
    assert_eq!(
        (r3.width_px, r3.height_px),
        (842, 595),
        "rotated render swaps axes"
    );
}

#[test]
fn extracts_text_and_searches() {
    let e = engine();
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    let t = e.text(info.id, 0).unwrap();
    assert!(t.text.contains("Dummy PDF file"), "text was {:?}", t.text);
    assert!(!t.chars.is_empty());
    assert!(t.chars.iter().any(|c| c.w > 0.0 && c.h > 0.0));
    let hits = e.search(info.id, "dummy".into(), false, false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page_index, 0);
    assert_eq!(hits[0].len, 5);
    assert_eq!(hits[0].rects.len(), 1, "single-line hit yields one rect");
    assert!(hits[0].rects[0].w > 0.0);
    assert_eq!(
        e.search(info.id, "Dummy".into(), true, false)
            .unwrap()
            .len(),
        1
    );
    assert!(e
        .search(info.id, "dummy".into(), true, false)
        .unwrap()
        .is_empty());
    assert_eq!(
        e.search(info.id, "umm".into(), false, true).unwrap().len(),
        0,
        "whole word"
    );
}

#[test]
fn reads_multi_page_document_and_outline() {
    let e = engine();
    let info = e.open(fixtures().join("outline.pdf"), None).unwrap();
    assert!(info.page_count > 1);
    let outline = e.outline(info.id).unwrap();
    let resolved = outline.iter().filter(|n| n.page_index.is_some()).count();
    println!("outline nodes: {} resolved: {}", outline.len(), resolved);
    let last = info.page_count - 1;
    let r = e.render(info.id, last, 0.5, 0).unwrap();
    assert!(r.width_px > 0);
    assert!(e.render(info.id, last + 1, 1.0, 0).is_err());
}

#[test]
fn annotation_crud_undo_redo_and_save_roundtrip() {
    let e = engine();
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    let id = info.id;
    assert!(e.list_annotations(id, 0).unwrap().is_empty());

    // Square
    let sq = e
        .add_annotation(
            id,
            0,
            spec(
                AnnotKind::Square,
                Rect {
                    x: 100.0,
                    y: 600.0,
                    w: 120.0,
                    h: 60.0,
                },
            ),
        )
        .unwrap();
    assert_eq!(sq.kind, AnnotKind::Square);
    assert_eq!(sq.author, "Test");
    assert_eq!(sq.color, Some(Color { r: 255, g: 0, b: 0 }));
    assert!((sq.rect.w - 120.0).abs() < 0.01);
    assert!(sq.modified.starts_with("D:20"));

    // Highlight with quads
    let mut hl = spec(
        AnnotKind::Highlight,
        Rect {
            x: 50.0,
            y: 700.0,
            w: 200.0,
            h: 14.0,
        },
    );
    hl.color = Some(Color {
        r: 255,
        g: 255,
        b: 0,
    });
    hl.quads = vec![[50.0, 714.0, 250.0, 714.0, 50.0, 700.0, 250.0, 700.0]];
    let hl = e.add_annotation(id, 0, hl).unwrap();
    assert_eq!(hl.quads.len(), 1);

    // Ink
    let mut ink = spec(
        AnnotKind::Ink,
        Rect {
            x: 300.0,
            y: 300.0,
            w: 100.0,
            h: 100.0,
        },
    );
    ink.ink = vec![vec![[300.0, 300.0], [350.0, 380.0], [400.0, 300.0]]];
    let ink = e.add_annotation(id, 0, ink).unwrap();
    assert_eq!(ink.ink.len(), 1);
    assert_eq!(ink.ink[0].len(), 3);

    // Sticky note and free text
    let mut note = spec(
        AnnotKind::Text,
        Rect {
            x: 500.0,
            y: 780.0,
            w: 20.0,
            h: 20.0,
        },
    );
    note.contents = "Hello note".into();
    let note = e.add_annotation(id, 0, note).unwrap();
    assert_eq!(note.contents, "Hello note");
    let mut ft = spec(
        AnnotKind::FreeText,
        Rect {
            x: 100.0,
            y: 400.0,
            w: 200.0,
            h: 50.0,
        },
    );
    ft.contents = "Free text (with parens)".into();
    let ft = e.add_annotation(id, 0, ft).unwrap();
    assert_eq!(ft.kind, AnnotKind::FreeText);

    let list = e.list_annotations(id, 0).unwrap();
    assert_eq!(list.len(), 5);
    let info = e.info(id).unwrap();
    assert!(info.modified && info.can_undo && !info.can_redo);

    // Rendering with annotations differs from the blank page.
    let blank_len = {
        let e2 = e.open(fixtures().join("sample.pdf"), None).unwrap();
        let r = e.render(e2.id, 0, 1.0, 0).unwrap();
        e.close(e2.id).unwrap();
        r.png_base64.len()
    };
    let annotated = e.render(id, 0, 1.0, 0).unwrap();
    assert_ne!(
        annotated.png_base64.len(),
        blank_len,
        "annotations should change the raster"
    );
    // Highlight quad spans x 50..250 at PDF y 700..714 => raster y = 842-707 = 135.
    let px = pixel(&annotated.png_base64, 150, 135);
    assert!(
        px[0] > 200 && px[1] > 200 && px[2] < 120,
        "highlight should paint yellow, got {px:?}"
    );
    // Square stroke at left edge x=100, y from 600..660 => raster y ~ 212.
    let edge = pixel(&annotated.png_base64, 100, 212);
    assert!(
        edge[0] > 150 && edge[1] < 100 && edge[2] < 100,
        "square stroke should be red, got {edge:?}"
    );

    // Update
    let upd = e
        .update_annotation(
            id,
            0,
            sq.index,
            AnnotationPatch {
                rect: Some(Rect {
                    x: 110.0,
                    y: 610.0,
                    w: 90.0,
                    h: 40.0,
                }),
                contents: Some("edited".into()),
                author: None,
                color: Some(Color { r: 0, g: 0, b: 255 }),
                interior_color: None,
                border_width: Some(4.0),
                hidden: None,
            },
        )
        .unwrap();
    assert_eq!(upd.contents, "edited");
    assert_eq!(upd.color, Some(Color { r: 0, g: 0, b: 255 }));
    assert!((upd.rect.w - 90.0).abs() < 0.01);
    assert!((upd.border_width - 4.0).abs() < 0.01);

    // Delete, then undo the delete, then redo it.
    e.delete_annotation(id, 0, ft.index).unwrap();
    assert_eq!(e.list_annotations(id, 0).unwrap().len(), 4);
    let after_undo = e.undo(id).unwrap();
    assert!(after_undo.can_redo);
    assert_eq!(e.list_annotations(id, 0).unwrap().len(), 5);
    e.redo(id).unwrap();
    assert_eq!(e.list_annotations(id, 0).unwrap().len(), 4);

    // Save As and reopen: annotations persist.
    let out = scratch("roundtrip.pdf");
    let saved = e
        .save(
            id,
            SaveOptions {
                path: Some(out.to_string_lossy().into_owned()),
                flatten: false,
            },
        )
        .unwrap();
    assert!(!saved.modified);
    assert_eq!(saved.path, out.to_string_lossy());
    assert!(out.exists());
    e.close(id).unwrap();
    let re = e.open(out.clone(), None).unwrap();
    let list = e.list_annotations(re.id, 0).unwrap();
    assert_eq!(list.len(), 4, "annotations survive save and reopen");
    let sq2 = list.iter().find(|a| a.kind == AnnotKind::Square).unwrap();
    assert_eq!(sq2.contents, "edited");
    assert_eq!(sq2.author, "Test");
    let ink2 = list.iter().find(|a| a.kind == AnnotKind::Ink).unwrap();
    assert_eq!(ink2.ink[0].len(), 3);

    // Flatten: annotations become page content and disappear from the list.
    let flat = scratch("flat.pdf");
    e.save(
        re.id,
        SaveOptions {
            path: Some(flat.to_string_lossy().into_owned()),
            flatten: true,
        },
    )
    .unwrap();
    e.close(re.id).unwrap();
    let fl = e.open(flat, None).unwrap();
    assert!(
        e.list_annotations(fl.id, 0).unwrap().is_empty(),
        "flattened file has no annots"
    );
    let r = e.render(fl.id, 0, 1.0, 0).unwrap();
    assert_ne!(
        r.png_base64.len(),
        blank_len,
        "flattened content still drawn"
    );
}

#[test]
fn line_rects_groups_by_line() {
    use sheaf_lib::engine::{line_rects, TextChar};
    let c = |x: f32, y: f32| TextChar {
        ch: "a".into(),
        x,
        y,
        w: 5.0,
        h: 10.0,
    };
    let r = line_rects(&[
        c(0.0, 100.0),
        c(5.0, 100.0),
        c(10.0, 100.5),
        c(0.0, 80.0),
        c(5.0, 80.0),
    ]);
    assert_eq!(r.len(), 2);
    assert!((r[0].w - 15.0).abs() < 0.01);
    assert!((r[1].w - 10.0).abs() < 0.01);
}

#[test]
fn form_fields_list_fill_and_xfdf_roundtrip() {
    let e = engine();
    let info = e.open(fixtures().join("form.pdf"), None).unwrap();

    // ---- discovery ----
    let fields = e.list_form_fields(info.id, 0).unwrap();
    let kinds: Vec<_> = fields.iter().map(|f| f.kind.as_str()).collect();
    assert!(kinds.contains(&"text"), "text field found: {kinds:?}");
    assert!(kinds.contains(&"checkbox"), "checkbox found");
    assert!(kinds.contains(&"radio"), "radio found");
    assert!(kinds.contains(&"combo"), "combo found");
    assert!(kinds.contains(&"listbox"), "listbox found");
    let name = fields
        .iter()
        .find(|f| f.name == "name")
        .expect("name field");
    assert!(name.required, "name is flagged required");
    let comments = fields.iter().find(|f| f.name == "comments").unwrap();
    assert!(comments.multiline);

    // ---- fill: text ----
    let idx = name.annot_index;
    let updated = e
        .set_form_field_value(info.id, 0, idx, "Brian Haywood".into())
        .unwrap();
    assert_eq!(updated.value, "Brian Haywood");

    // ---- fill: checkbox on/off ----
    let cb = fields.iter().find(|f| f.kind == "checkbox").unwrap();
    let on = e
        .set_form_field_value(info.id, 0, cb.annot_index, "on".into())
        .unwrap();
    assert!(on.checked, "checkbox turned on");
    let off = e
        .set_form_field_value(info.id, 0, cb.annot_index, "off".into())
        .unwrap();
    assert!(!off.checked, "checkbox turned off");
    let on2 = e
        .set_form_field_value(info.id, 0, cb.annot_index, "on".into())
        .unwrap();
    assert!(on2.checked);

    // ---- fill: radio group ----
    let green = fields
        .iter()
        .find(|f| f.kind == "radio" && f.export_value == "green")
        .expect("green radio");
    let g = e
        .set_form_field_value(info.id, 0, green.annot_index, "on".into())
        .unwrap();
    assert!(g.checked, "green selected");
    let after: Vec<_> = e
        .list_form_fields(info.id, 0)
        .unwrap()
        .into_iter()
        .filter(|f| f.kind == "radio")
        .collect();
    assert_eq!(
        after.iter().filter(|f| f.checked).count(),
        1,
        "radio group is exclusive"
    );

    // ---- fill: combo ----
    let combo = fields.iter().find(|f| f.kind == "combo").unwrap();
    let c = e
        .set_form_field_value(info.id, 0, combo.annot_index, "Large".into())
        .unwrap();
    assert_eq!(c.value, "large", "combo stores the option export value");

    // ---- persistence through save ----
    let out = std::env::temp_dir().join(format!("sheaf-form-{}.pdf", std::process::id()));
    e.save_copy(info.id, out.clone()).unwrap();
    let reopened = e.open(out.clone(), None).unwrap();
    let saved = e.list_form_fields(reopened.id, 0).unwrap();
    assert_eq!(
        saved.iter().find(|f| f.name == "name").unwrap().value,
        "Brian Haywood",
        "text value survives save"
    );
    assert!(
        saved
            .iter()
            .find(|f| f.kind == "radio" && f.export_value == "green")
            .unwrap()
            .checked,
        "radio survives save"
    );
    e.close(reopened.id).unwrap();
    let _ = std::fs::remove_file(out);

    // ---- undo restores previous value ----
    e.undo(info.id).unwrap(); // undo combo set
    let undone = e.list_form_fields(info.id, 0).unwrap();
    assert_ne!(
        undone.iter().find(|f| f.kind == "combo").unwrap().value,
        "Large",
        "undo reverted combo"
    );

    // ---- XFDF export ----
    let xml = e.export_xfdf(info.id).unwrap();
    assert!(xml.contains("name=\"name\""), "xfdf has name field: {xml}");
    assert!(xml.contains("Brian Haywood"), "xfdf carries value");
    assert!(xml.contains("name=\"color\""), "radio group exported");

    // ---- XFDF import into a fresh copy ----
    let fresh = e.open(fixtures().join("form.pdf"), None).unwrap();
    let n = e.import_xfdf(fresh.id, xml).unwrap();
    assert!(n >= 3, "several fields applied, got {n}");
    let imported = e.list_form_fields(fresh.id, 0).unwrap();
    assert_eq!(
        imported.iter().find(|f| f.name == "name").unwrap().value,
        "Brian Haywood",
        "import applied text value"
    );
    assert!(
        imported
            .iter()
            .find(|f| f.kind == "radio" && f.export_value == "green")
            .unwrap()
            .checked,
        "import applied radio selection"
    );
    e.close(fresh.id).unwrap();
    e.close(info.id).unwrap();
}

#[test]
fn page_organization_ops() {
    let e = engine();
    let info = e.open(fixtures().join("outline.pdf"), None).unwrap();
    let n0 = info.page_count;
    assert!(n0 >= 4, "fixture has enough pages ({n0})");

    // ---- rotate ----
    let info = e.rotate_pages(info.id, vec![0, 1], 90).unwrap();
    assert_eq!(info.pages[0].rotation, 90, "page 0 rotated");
    assert_eq!(info.pages[1].rotation, 90, "page 1 rotated");
    assert_eq!(info.pages[2].rotation, 0, "page 2 untouched");
    let info = e.rotate_pages(info.id, vec![0], -90).unwrap();
    assert_eq!(info.pages[0].rotation, 0, "counter-rotation");

    // ---- move: page 1 to the front ----
    let h1_before = info.pages[1].height;
    let info = e.move_pages(info.id, vec![1], 0).unwrap();
    assert_eq!(info.page_count, n0);
    assert_eq!(info.pages[0].rotation, 90, "rotated page moved to front");
    assert_eq!(info.pages[0].height, h1_before);

    // ---- delete ----
    let info = e.delete_pages(info.id, vec![0]).unwrap();
    assert_eq!(info.page_count, n0 - 1, "one page deleted");
    assert_eq!(
        info.pages[0].rotation, 0,
        "original page 0 is back at front"
    );

    // ---- undo restores the deleted page ----
    let info = e.undo(info.id).unwrap();
    assert_eq!(info.page_count, n0, "undo restored the page");
    let info = e.undo(info.id).unwrap(); // undo the move too
    assert_eq!(info.pages[1].rotation, 90, "move undone");

    // ---- extract (split) ----
    let out = std::env::temp_dir().join(format!("sheaf-extract-{}.pdf", std::process::id()));
    e.extract_pages(info.id, vec![0, 2], out.clone()).unwrap();
    let split = e.open(out.clone(), None).unwrap();
    assert_eq!(split.page_count, 2, "extracted two pages");
    e.close(split.id).unwrap();

    // ---- insert (merge) ----
    let before = info.page_count;
    let info = e.insert_pages(info.id, out.clone(), None, before).unwrap();
    assert_eq!(
        info.page_count,
        before + 2,
        "two pages merged in at the end"
    );
    let _ = std::fs::remove_file(out);

    // ---- crop ----
    let orig_w = info.pages[0].width;
    let info = e
        .crop_pages(
            info.id,
            vec![0],
            [72.0, 72.0, orig_w - 72.0, info.pages[0].height - 72.0],
        )
        .unwrap();
    assert!(
        (info.pages[0].width - (orig_w - 144.0)).abs() < 1.0,
        "crop narrowed page 0: {} -> {}",
        orig_w,
        info.pages[0].width
    );

    e.close(info.id).unwrap();
}

#[test]
fn stamping_headers_bates_watermark() {
    let e = engine();
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();

    let spec = |text: &str, position: &str| sheaf_lib::engine::StampSpec {
        pages: vec![],
        text: text.into(),
        position: position.into(),
        font_size: 12.0,
        color: Color { r: 200, g: 0, b: 0 },
        opacity: 255,
        start_at: 1,
        bates_digits: 6,
    };

    // Header with page numbers.
    let info2 = e
        .stamp_pages(info.id, spec("Page {n} of {total}", "header-center"))
        .unwrap();
    assert!(info2.modified);
    let t = e.text(info.id, 0).unwrap();
    assert!(
        t.text.contains(&format!("Page 1 of {}", info.page_count)),
        "header text present on page 0: {:?}",
        &t.text[..t.text.len().min(120)]
    );

    // Bates numbering in the footer.
    e.stamp_pages(info.id, spec("BATES-{bates}", "footer-right"))
        .unwrap();
    let t = e.text(info.id, 0).unwrap();
    assert!(t.text.contains("BATES-000001"), "bates on page 0");
    if info.page_count > 1 {
        let t1 = e.text(info.id, 1).unwrap();
        assert!(t1.text.contains("BATES-000002"), "bates increments");
    }

    // Watermark renders (visible ink on the page bitmap).
    let before = e.render(info.id, 0, 0.5, 0).unwrap().png_base64.len();
    e.stamp_pages(
        info.id,
        sheaf_lib::engine::StampSpec {
            opacity: 60,
            font_size: 48.0,
            ..spec("CONFIDENTIAL", "watermark")
        },
    )
    .unwrap();
    let after = e.render(info.id, 0, 0.5, 0).unwrap().png_base64.len();
    assert_ne!(before, after, "watermark changed the rendered page");

    // Undo unwinds the watermark.
    let u = e.undo(info.id).unwrap();
    assert!(u.can_redo);
    let t = e.text(info.id, 0).unwrap();
    assert!(!t.text.contains("CONFIDENTIAL"), "watermark undone");

    e.close(info.id).unwrap();
}

// ---------- M5: sign and protect through the engine ----------

#[test]
fn m5_sign_save_reopen_annotate_keeps_signature() {
    use sheaf_lib::security::{IdentityStore, SecuritySpec, SignSpec};
    let e = engine();
    let ids = std::env::temp_dir().join(format!("sheaf-engine-ids-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ids);
    let idn = IdentityStore::new(ids.clone())
        .create_self_signed("Engine Signer", Some("Sheaf"), "")
        .unwrap();

    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    let id = info.id;
    assert!(e.list_signatures(id).unwrap().is_empty());

    // Sign with a visible appearance on page 1.
    let spec = SignSpec {
        identity_id: idn.id.clone(),
        password: String::new(),
        page: 0,
        rect: [72.0, 72.0, 300.0, 140.0],
        reason: Some("Engine test".into()),
        location: None,
        contact: None,
        name: None,
        lock: false,
    };
    let info = e.sign_document(id, ids.clone(), spec).unwrap();
    assert!(info.modified && info.can_undo);
    let sigs = e.list_signatures(id).unwrap();
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].status, "valid", "{:?}", sigs[0]);

    // The appearance renders: sample the tinted box.
    let png = e.render(id, 0, 1.0, 0).unwrap();
    let px = pixel(&png.png_base64, 100, (png.height_px as f32 - 100.0) as u32);
    assert!(px[2] > px[0], "expected the bluish signature tint, got {px:?}");

    // Save, reopen, still valid.
    let out = scratch("m5-signed.pdf");
    e.save(id, SaveOptions { path: Some(out.to_string_lossy().into_owned()), flatten: false }).unwrap();
    e.close(id).unwrap();
    let info = e.open(out.clone(), None).unwrap();
    let id = info.id;
    let sigs = e.list_signatures(id).unwrap();
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].status, "valid", "{:?}", sigs[0]);
    assert!(sigs[0].covers_whole_document);

    // Annotate after signing and save: the signature must stay valid but no
    // longer cover the whole file (incremental update).
    e.add_annotation(id, 0, spec_annot(AnnotKind::Square, Rect { x: 10.0, y: 10.0, w: 50.0, h: 50.0 })).unwrap();
    let out2 = scratch("m5-signed-annotated.pdf");
    e.save(id, SaveOptions { path: Some(out2.to_string_lossy().into_owned()), flatten: false }).unwrap();
    let sigs = e.list_signatures(id).unwrap();
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].status, "modified", "{:?}", sigs[0]);
    assert!(!sigs[0].covers_whole_document);
    let bytes = std::fs::read(&out2).unwrap();
    let signed = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..signed.len()], &signed[..], "save after signing must be incremental");
    e.close(id).unwrap();

    // Protect: reopen requires the password; permissions are recorded.
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    let id = info.id;
    let spec = SecuritySpec {
        user_password: "pw".into(),
        owner_password: "own".into(),
        allow_print: false,
        allow_print_high_quality: false,
        allow_modify: false,
        allow_copy: false,
        allow_annotate: true,
        allow_fill_forms: true,
        allow_assemble: false,
        allow_accessibility: true,
    };
    let info = e.protect(id, spec).unwrap();
    assert!(info.encrypted, "{info:?}");
    assert_eq!(info.permissions & 0b100, 0, "print bit should be clear");
    let out3 = scratch("m5-protected.pdf");
    e.save(id, SaveOptions { path: Some(out3.to_string_lossy().into_owned()), flatten: false }).unwrap();
    e.close(id).unwrap();
    let err = e.open(out3.clone(), None).unwrap_err();
    assert!(err.to_string().contains("password"), "{err}");
    let info = e.open(out3.clone(), Some("pw".into())).unwrap();
    assert!(info.encrypted);
    assert_eq!(info.page_count, 1);
    // Remove security, save, reopen without password.
    let info = e.unprotect(info.id).unwrap();
    assert!(!info.encrypted);
    let out4 = scratch("m5-unprotected.pdf");
    e.save(info.id, SaveOptions { path: Some(out4.to_string_lossy().into_owned()), flatten: false }).unwrap();
    e.close(info.id).unwrap();
    let info = e.open(out4, None).unwrap();
    assert!(!info.encrypted);
    e.close(info.id).unwrap();
    let _ = std::fs::remove_dir_all(&ids);
}

// ---------- M6: content editing ----------

#[test]
fn m6_edit_text_images_links_create_export() {
    use sheaf_lib::edit::{ImageSpec, LinkSpec};
    let e = engine();
    let info = e.open(fixtures().join("sample.pdf"), None).unwrap();
    let id = info.id;

    // Objects on page 1: at least one text run with the heading.
    let objs = e.list_page_objects(id, 0).unwrap();
    let text_objs: Vec<_> = objs.iter().filter(|o| o.kind == "text").collect();
    assert!(!text_objs.is_empty(), "{objs:?}");
    let heading = text_objs
        .iter()
        .find(|o| o.text.as_deref().map(|t| t.starts_with("Dumm")).unwrap_or(false))
        .expect("heading text object (runs may split words)");
    assert!(heading.font_size.unwrap() > 5.0);
    assert!(heading.font.is_some());

    // Edit the run's text and confirm the page text changed.
    let info = e.set_text_object(id, 0, heading.index, "Edited by Sheaf".into(), None).unwrap();
    assert!(info.can_undo);
    let txt = e.text(id, 0).unwrap().text;
    assert!(txt.contains("Edited by Sheaf"), "{txt}");
    assert!(!txt.contains("Dumm"), "{txt}");

    // Move it and confirm the bounds shifted.
    let before = e.list_page_objects(id, 0).unwrap()[heading.index as usize].rect;
    e.move_page_object(id, 0, heading.index, 50.0, -30.0, 1.0).unwrap();
    let after = e.list_page_objects(id, 0).unwrap()[heading.index as usize].rect;
    assert!((after.x - before.x - 50.0).abs() < 0.5 && (after.y - before.y + 30.0).abs() < 0.5, "{before:?} -> {after:?}");

    // Insert a PNG image and see its pixels on the page.
    let img_path = scratch("m6-red.png");
    let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([220, 20, 20, 255]));
    img.save(&img_path).unwrap();
    let n_before = e.list_page_objects(id, 0).unwrap().len();
    e.insert_image(id, 0, ImageSpec { path: img_path.to_string_lossy().into_owned(), rect: Rect { x: 100.0, y: 100.0, w: 200.0, h: 0.0 } }).unwrap();
    let objs = e.list_page_objects(id, 0).unwrap();
    assert_eq!(objs.len(), n_before + 1);
    let im = objs.iter().find(|o| o.kind == "image").unwrap();
    assert_eq!((im.image_width, im.image_height), (Some(40), Some(20)));
    assert!((im.rect.w - 200.0).abs() < 1.0 && (im.rect.h - 100.0).abs() < 1.0, "{:?}", im.rect);
    let png = e.render(id, 0, 1.0, 0).unwrap();
    let px = pixel(&png.png_base64, 200, png.height_px - 150);
    assert!(px[0] > 180 && px[1] < 80, "expected red image pixel, got {px:?}");

    // Extract that image back out as PNG.
    let out_png = scratch("m6-extracted.png");
    e.extract_image(id, 0, im.index, out_png.clone()).unwrap();
    let back = image::open(&out_png).unwrap().into_rgba8();
    assert_eq!(back.dimensions(), (40, 20));
    assert!(back.get_pixel(5, 5).0[0] > 180);

    // Delete the image.
    e.delete_page_object(id, 0, im.index).unwrap();
    assert_eq!(e.list_page_objects(id, 0).unwrap().len(), n_before);

    // Add a brand-new text run in a standard font.
    e.add_text(id, 0, sheaf_lib::edit::TextSpec { text: "Added run".into(), x: 72.0, y: 400.0, font: "Helvetica".into(), font_size: 14.0, color: Some(Color { r: 0, g: 0, b: 200 }) }).unwrap();
    assert!(e.text(id, 0).unwrap().text.contains("Added run"));
    assert!(e.list_page_objects(id, 0).unwrap().iter().any(|o| o.text.as_deref() == Some("Added run") && (o.rect.y - 400.0).abs() < 5.0));

    // Links: add a URI link and read it back.
    assert!(e.list_links(id, 0).unwrap().is_empty());
    e.add_link(id, 0, LinkSpec { rect: Rect { x: 50.0, y: 700.0, w: 100.0, h: 20.0 }, uri: Some("https://example.org".into()), page: None }).unwrap();
    let links = e.list_links(id, 0).unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].uri.as_deref(), Some("https://example.org"));
    assert!((links[0].rect.x - 50.0).abs() < 0.5);

    // Save, reopen, edits and link persist.
    let out = scratch("m6-edited.pdf");
    e.save(id, SaveOptions { path: Some(out.to_string_lossy().into_owned()), flatten: false }).unwrap();
    e.close(id).unwrap();
    let info = e.open(out, None).unwrap();
    assert!(e.text(info.id, 0).unwrap().text.contains("Edited by Sheaf"));
    assert_eq!(e.list_links(info.id, 0).unwrap().len(), 1);

    // Export to images and text.
    let dir = std::env::temp_dir().join("sheaf-tests").join("m6-export");
    let files = e.export_images(info.id, vec![0], dir.clone(), 96.0).unwrap();
    assert_eq!(files.len(), 1);
    let exported = image::open(&files[0]).unwrap();
    assert_eq!(exported.width(), (595.0f32 * 96.0 / 72.0).round() as u32);
    let txt = e.export_text(info.id, vec![0]).unwrap();
    assert!(txt.contains("Edited by Sheaf"));
    e.close(info.id).unwrap();

    // Create a PDF from two images.
    let img2 = scratch("m6-blue.png");
    image::RgbaImage::from_pixel(300, 150, image::Rgba([20, 20, 220, 255])).save(&img2).unwrap();
    let created = scratch("m6-from-images.pdf");
    let info = e.create_from_images(vec![img_path.clone(), img2.clone()], created.clone()).unwrap();
    assert_eq!(info.page_count, 2);
    assert!((info.pages[1].width - 300.0).abs() < 0.5 && (info.pages[1].height - 150.0).abs() < 0.5, "{:?}", info.pages[1]);
    let png = e.render(info.id, 1, 1.0, 0).unwrap();
    let px = pixel(&png.png_base64, 150, 75);
    assert!(px[2] > 180 && px[0] < 80, "expected blue, got {px:?}");
    e.close(info.id).unwrap();
}
