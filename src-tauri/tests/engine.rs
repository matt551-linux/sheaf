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
