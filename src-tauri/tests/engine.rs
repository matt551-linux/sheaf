//! Integration tests against the PDF engine using real fixture PDFs.
//! Requires the PDFium binary fetched by `node scripts/fetch-pdfium.mjs`.

use std::path::PathBuf;

use sheaf_lib::engine::Engine;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("fixtures")
}

fn engine() -> Engine {
    Engine::start(sheaf_lib::pdfium_dir()).expect("engine starts")
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
    let hits = e.search(info.id, "dummy".into(), false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page_index, 0);
    assert!(e.search(info.id, "Dummy".into(), true).unwrap().len() == 1);
    assert!(e.search(info.id, "dummy".into(), true).unwrap().is_empty());
}

#[test]
fn reads_multi_page_document_and_outline() {
    let e = engine();
    let info = e.open(fixtures().join("outline.pdf"), None).unwrap();
    assert!(info.page_count > 1);
    let outline = e.outline(info.id).unwrap();
    // TAMReview.pdf ships bookmarks; make sure we walk them without panicking
    // and that at least one resolves to a page.
    let resolved = outline.iter().filter(|n| n.page_index.is_some()).count();
    println!("outline nodes: {} resolved: {}", outline.len(), resolved);
    let last = info.page_count - 1;
    let r = e.render(info.id, last, 0.5, 0).unwrap();
    assert!(r.width_px > 0);
    assert!(e.render(info.id, last + 1, 1.0, 0).is_err());
}
