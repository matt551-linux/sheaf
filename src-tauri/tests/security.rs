//! M5 security tests: identities, signing, verifying, protecting. These do not
//! need PDFium; they work on bytes.

use std::path::PathBuf;

use sheaf_lib::security::{
    list_signatures, protect, sign, unprotect, IdentityStore, SecuritySpec, SignSpec,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("fixtures")
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sheaf-sec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample() -> Vec<u8> {
    std::fs::read(fixtures().join("sample.pdf")).unwrap()
}

#[test]
fn identity_create_list_delete() {
    let dir = temp_dir("ids");
    let store = IdentityStore::new(dir.clone());
    assert!(store.list().unwrap().is_empty());
    let idn = store
        .create_self_signed("Test Person", Some("Sheaf Tests"), "pw")
        .unwrap();
    assert!(idn.self_signed);
    assert!(idn.subject.contains("Test Person"), "{}", idn.subject);
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, idn.id);
    store.delete(&idn.id).unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn identity_wrong_password_is_reported() {
    let dir = temp_dir("idpw");
    let store = IdentityStore::new(dir);
    let idn = store.create_self_signed("P", None, "right").unwrap();
    let spec = SignSpec {
        identity_id: idn.id.clone(),
        password: "wrong".into(),
        page: 0,
        rect: [0.0; 4],
        reason: None,
        location: None,
        contact: None,
        name: None,
        lock: false,
    };
    let err = sign(&sample(), None, &store, &spec).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("password") || msg.contains("pkcs12") || msg.contains("mac"),
        "{msg}"
    );
}

#[test]
fn sign_verify_tamper_and_second_signature() {
    let dir = temp_dir("sign");
    let store = IdentityStore::new(dir);
    let idn = store.create_self_signed("Signer One", None, "").unwrap();
    let src = sample();

    let spec = SignSpec {
        identity_id: idn.id.clone(),
        password: String::new(),
        page: 0,
        rect: [72.0, 72.0, 300.0, 140.0],
        reason: Some("Approved".into()),
        location: Some("Test".into()),
        contact: None,
        name: None,
        lock: false,
    };
    let signed = sign(&src, None, &store, &spec).unwrap();
    // Incremental update: original bytes are a prefix.
    assert_eq!(&signed[..src.len()], &src[..]);
    assert!(signed.len() > src.len() + 20_000);

    let sigs = list_signatures(&signed, None).unwrap();
    assert_eq!(sigs.len(), 1, "{sigs:?}");
    let s = &sigs[0];
    assert_eq!(s.status, "valid", "{s:?}");
    assert!(s.covers_whole_document);
    assert_eq!(s.signer, "Signer One");
    assert_eq!(s.reason.as_deref(), Some("Approved"));
    assert_eq!(s.page, Some(0));
    assert!(s.self_signed);
    assert_eq!(s.rect.map(|r| r[0] as i32), Some(72));

    // Tamper with a byte inside the signed range.
    let mut bad = signed.clone();
    bad[100] ^= 0x55;
    let sigs = list_signatures(&bad, None).unwrap();
    assert_eq!(sigs[0].status, "invalid", "{:?}", sigs[0]);

    // A second signature keeps the first valid but it no longer covers the whole file.
    let idn2 = store.create_self_signed("Signer Two", None, "").unwrap();
    let spec2 = SignSpec {
        identity_id: idn2.id,
        password: String::new(),
        page: 0,
        rect: [320.0, 72.0, 540.0, 140.0],
        reason: None,
        location: None,
        contact: None,
        name: Some("S. Two".into()),
        lock: true,
    };
    let twice = sign(&signed, None, &store, &spec2).unwrap();
    assert_eq!(&twice[..signed.len()], &signed[..]);
    let sigs = list_signatures(&twice, None).unwrap();
    assert_eq!(sigs.len(), 2, "{sigs:?}");
    let one = sigs.iter().find(|s| s.signer == "Signer One").unwrap();
    let two = sigs.iter().find(|s| s.signer == "S. Two").unwrap();
    assert_eq!(one.status, "modified", "{one:?}");
    assert_eq!(two.status, "valid", "{two:?}");
    assert!(two.covers_whole_document);
    assert!(two.locks_document);
    assert!(!one.locks_document);

    // Structural sanity: the appended data has an AcroForm with SigFlags and both fields.
    let tail = String::from_utf8_lossy(&twice[src.len()..]);
    assert!(tail.contains("/SigFlags 3"));
    assert!(tail.contains("/DocMDP"));
    assert!(tail.contains("/FT /Sig") || tail.contains("/FT/Sig"), "{}", &tail[..2000]);
}

#[test]
fn protect_and_unprotect_roundtrip() {
    let src = sample();
    let spec = SecuritySpec {
        user_password: "open-me".into(),
        owner_password: "owner".into(),
        allow_print: true,
        allow_print_high_quality: false,
        allow_modify: false,
        allow_copy: false,
        allow_annotate: true,
        allow_fill_forms: true,
        allow_assemble: false,
        allow_accessibility: true,
    };
    let enc = protect(&src, None, &spec).unwrap();
    let head = String::from_utf8_lossy(&enc[..1024]);
    assert!(head.starts_with("%PDF-1.7"), "{head}");
    assert!(enc.windows(8).any(|w| w == b"/Encrypt"));
    let text = String::from_utf8_lossy(&enc);
    let i = text.find("/Filter /Standard").or_else(|| text.find("/Filter/Standard")).expect("no /Encrypt dictionary");
    let dict = &text[i..(i + 600).min(text.len())];
    let norm = dict.replace(' ', "").replace('\n', "");
    assert!(norm.contains("/V5") && norm.contains("/R6") && norm.contains("/CFM/AESV3"), "expected AES-256 V5/R6: {dict}");
    // Text of the sample must not be visible in the clear.
    assert!(!enc.windows(5).any(|w| w == b"Lorem") || !src.windows(5).any(|w| w == b"Lorem"));

    // lopdf must refuse the wrong password and accept the right one.
    assert!(lopdf::Document::load_mem_with_options(&enc, lopdf::LoadOptions::with_password("nope")).is_err());
    let d = lopdf::Document::load_mem_with_options(&enc, lopdf::LoadOptions::with_password("open-me")).unwrap();
    assert!(!d.get_pages().is_empty());
    let d = lopdf::Document::load_mem_with_options(&enc, lopdf::LoadOptions::with_password("owner")).unwrap();
    assert!(!d.get_pages().is_empty());

    // Remove security.
    let plain = unprotect(&enc, Some("open-me")).unwrap();
    assert!(!plain.windows(8).any(|w| w == b"/Encrypt"));
    let d = lopdf::Document::load_mem(&plain).unwrap();
    assert_eq!(d.get_pages().len(), lopdf::Document::load_mem(&src).unwrap().get_pages().len());

    // Wrong current password fails.
    assert!(unprotect(&enc, Some("bad")).is_err());

    // Signing an encrypted document with the password works and stays encrypted.
    let dir = temp_dir("signenc");
    let store = IdentityStore::new(dir);
    let idn = store.create_self_signed("Enc Signer", None, "").unwrap();
    let spec = SignSpec {
        identity_id: idn.id,
        password: String::new(),
        page: 0,
        rect: [0.0; 4],
        reason: None,
        location: None,
        contact: None,
        name: None,
        lock: false,
    };
    let signed_enc = sign(&enc, Some("open-me"), &store, &spec).unwrap();
    assert_eq!(&signed_enc[..enc.len()], &enc[..]);
    let sigs = list_signatures(&signed_enc, Some("open-me")).unwrap();
    assert_eq!(sigs.len(), 1, "{sigs:?}");
    assert_eq!(sigs[0].status, "valid", "{:?}", sigs[0]);
}
