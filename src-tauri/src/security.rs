//! M5: sign and protect.
//!
//! Everything here works on PDF bytes without PDFium:
//!
//! * **Identities**: self-signed RSA-2048 certificates generated locally, or
//!   PKCS#12 (`.p12`/`.pfx`) files imported by the user. Stored as PKCS#12
//!   under the app data dir, one file per identity, protected by a per-file
//!   password the user chooses (or empty for a self-signed test identity).
//! * **Signing**: a visible `/Sig` widget plus AcroForm entry appended as an
//!   incremental update with `lopdf`, then the `/ByteRange` gap is hashed and
//!   the CMS `SignedData` produced by `pdf_oxide`. The original bytes are
//!   never modified, so earlier signatures stay valid.
//! * **Verifying**: every `/Sig` reachable from the AcroForm is checked for
//!   ByteRange coverage and CMS/messageDigest validity.
//! * **Protecting**: AES-256 (V5/R6) password + permission encryption, and its
//!   removal, via `lopdf`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lopdf::{dictionary, Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SheafError};

fn pdf(msg: impl Into<String>) -> SheafError {
    SheafError::Pdf(msg.into())
}

// ---------- identities ----------

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    /// File stem under the identities dir (also the stable id).
    pub id: String,
    pub subject: String,
    pub issuer: String,
    pub self_signed: bool,
    pub not_after: String,
    pub path: String,
}

/// Loaded key material, ready to sign.
struct Credentials {
    cert_pem: String,
    key_pem: String,
}

fn der_to_pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap());
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// Generate an RSA-2048 self-signed certificate. Returns (cert DER, PKCS#8 key DER).
pub fn generate_self_signed(common_name: &str, org: Option<&str>, years: u32) -> Result<(Vec<u8>, Vec<u8>)> {
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
    use std::str::FromStr;
    use x509_cert::builder::{Builder, CertificateBuilder, Profile};
    use x509_cert::der::Encode;
    use x509_cert::name::Name;
    use x509_cert::serial_number::SerialNumber;
    use x509_cert::spki::SubjectPublicKeyInfoOwned;
    use x509_cert::time::Validity;

    let mut rng = rand::thread_rng();
    let key = rsa::RsaPrivateKey::new(&mut rng, 2048).map_err(|e| pdf(format!("keygen: {e}")))?;
    let key_der = key
        .to_pkcs8_der()
        .map_err(|e| pdf(format!("pkcs8: {e}")))?
        .as_bytes()
        .to_vec();
    let signer = SigningKey::<sha2::Sha256>::new(key.clone());
    let pub_der = key
        .to_public_key()
        .to_public_key_der()
        .map_err(|e| pdf(format!("spki: {e}")))?;
    let spki = SubjectPublicKeyInfoOwned::try_from(pub_der.as_bytes())
        .map_err(|e| pdf(format!("spki: {e}")))?;
    let esc = |s: &str| s.replace('\\', "\\\\").replace(',', "\\,").replace('=', "\\=");
    let mut dn = format!("CN={}", esc(common_name.trim()));
    if let Some(o) = org.map(str::trim).filter(|o| !o.is_empty()) {
        dn.push_str(&format!(",O={}", esc(o)));
    }
    let subject = Name::from_str(&dn).map_err(|e| pdf(format!("subject: {e}")))?;
    let serial = SerialNumber::from(rand::random::<u32>() as u64 | 1);
    let validity = Validity::from_now(std::time::Duration::from_secs(
        years.max(1) as u64 * 365 * 24 * 3600,
    ))
    .map_err(|e| pdf(format!("validity: {e}")))?;
    let builder = CertificateBuilder::new(Profile::Root, serial, validity, subject, spki, &signer)
        .map_err(|e| pdf(format!("cert: {e}")))?;
    let cert = builder
        .build::<rsa::pkcs1v15::Signature>()
        .map_err(|e| pdf(format!("cert: {e}")))?;
    let cert_der = cert.to_der().map_err(|e| pdf(format!("cert der: {e}")))?;
    Ok((cert_der, key_der))
}

fn describe_cert(der: &[u8]) -> Result<(String, String, bool, String)> {
    let (_, cert) = x509_parser::parse_x509_certificate(der)
        .map_err(|e| pdf(format!("certificate: {e}")))?;
    let subject = cert.subject().to_string();
    let issuer = cert.issuer().to_string();
    let not_after = cert.validity().not_after.to_rfc2822().unwrap_or_default();
    Ok((subject.clone(), issuer.clone(), subject == issuer, not_after))
}

/// Friendly display name: the CN if present, else the whole DN.
pub fn display_name(dn: &str) -> String {
    dn.split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("CN="))
        .map(|s| s.to_string())
        .unwrap_or_else(|| dn.to_string())
}

pub struct IdentityStore {
    dir: PathBuf,
}

impl IdentityStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        Ok(())
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.p12"))
    }

    fn write_p12(&self, id: &str, cert_der: &[u8], key_der: &[u8], chain: &[Vec<u8>], password: &str) -> Result<PathBuf> {
        use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
        self.ensure_dir()?;
        let key = PrivateKey::from_der(key_der).map_err(|e| pdf(format!("private key: {e}")))?;
        let mut certs = vec![Certificate::from_der(cert_der).map_err(|e| pdf(format!("certificate: {e}")))?];
        for c in chain {
            certs.push(Certificate::from_der(c).map_err(|e| pdf(format!("chain certificate: {e}")))?);
        }
        let local_key_id: Vec<u8> = {
            use sha2::Digest;
            sha2::Sha256::digest(cert_der)[..20].to_vec()
        };
        let chain = PrivateKeyChain::new(local_key_id, key, certs);
        let mut ks = KeyStore::new();
        ks.add_entry(id, KeyStoreEntry::PrivateKeyChain(chain));
        let bytes = ks
            .writer(password)
            .write()
            .map_err(|e| pdf(format!("pkcs12 write: {e}")))?;
        let path = self.path_for(id);
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    fn read(&self, id: &str, password: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<Vec<u8>>)> {
        let bytes = std::fs::read(self.path_for(id))?;
        load_pkcs12(&bytes, password)
    }

    pub fn list(&self) -> Result<Vec<Identity>> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Ok(out);
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("p12") {
                continue;
            }
            let id = p.file_stem().unwrap().to_string_lossy().into_owned();
            // Sidecar with the public description so listing never needs a password.
            let meta = p.with_extension("json");
            if let Ok(s) = std::fs::read_to_string(&meta) {
                if let Ok(mut idn) = serde_json::from_str::<IdentityMeta>(&s) {
                    idn.id = id.clone();
                    out.push(Identity {
                        id,
                        subject: idn.subject,
                        issuer: idn.issuer,
                        self_signed: idn.self_signed,
                        not_after: idn.not_after,
                        path: p.to_string_lossy().into_owned(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.subject.cmp(&b.subject));
        Ok(out)
    }

    fn write_meta(&self, id: &str, cert_der: &[u8]) -> Result<Identity> {
        let (subject, issuer, self_signed, not_after) = describe_cert(cert_der)?;
        let meta = IdentityMeta {
            id: id.to_string(),
            subject: subject.clone(),
            issuer: issuer.clone(),
            self_signed,
            not_after: not_after.clone(),
        };
        std::fs::write(
            self.path_for(id).with_extension("json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )?;
        Ok(Identity {
            id: id.to_string(),
            subject,
            issuer,
            self_signed,
            not_after,
            path: self.path_for(id).to_string_lossy().into_owned(),
        })
    }

    pub fn create_self_signed(&self, common_name: &str, org: Option<&str>, password: &str) -> Result<Identity> {
        let (cert, key) = generate_self_signed(common_name, org, 5)?;
        let id = format!("self-{}", nanos());
        self.write_p12(&id, &cert, &key, &[], password)?;
        self.write_meta(&id, &cert)
    }

    /// Import a `.p12`/`.pfx`. It is re-encrypted with `store_password` so the
    /// user can pick a different (or empty) password for day-to-day use.
    pub fn import_pkcs12(&self, file: &Path, file_password: &str, store_password: &str) -> Result<Identity> {
        let bytes = std::fs::read(file)?;
        let (cert, key, chain) = load_pkcs12(&bytes, file_password)?;
        let id = format!("import-{}", nanos());
        self.write_p12(&id, &cert, &key, &chain, store_password)?;
        self.write_meta(&id, &cert)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let p = self.path_for(id);
        let _ = std::fs::remove_file(p.with_extension("json"));
        std::fs::remove_file(p)?;
        Ok(())
    }

    fn credentials(&self, id: &str, password: &str) -> Result<Credentials> {
        let (cert, key, _chain) = self.read(id, password)?;
        Ok(Credentials {
            cert_pem: der_to_pem("CERTIFICATE", &cert),
            key_pem: der_to_pem("PRIVATE KEY", &key),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct IdentityMeta {
    #[serde(default)]
    id: String,
    subject: String,
    issuer: String,
    self_signed: bool,
    not_after: String,
}

/// (leaf cert DER, PKCS#8 key DER, chain DERs)
fn load_pkcs12(bytes: &[u8], password: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<Vec<u8>>)> {
    use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
    let ks = KeyStore::from_pkcs12(bytes, password, Pkcs12ImportPolicy::Relaxed).map_err(|e| {
        let m = e.to_string();
        if m.to_lowercase().contains("mac") || m.to_lowercase().contains("password") {
            SheafError::PasswordRequired
        } else {
            pdf(format!("pkcs12: {m}"))
        }
    })?;
    let (_alias, chain) = ks
        .private_key_chain()
        .ok_or_else(|| pdf("the PKCS#12 file has no private key with a certificate"))?;
    let certs = chain.certs();
    let leaf = certs.first().ok_or_else(|| pdf("no certificate in PKCS#12"))?;
    let rest = certs.iter().skip(1).map(|c| c.as_der().to_vec()).collect();
    Ok((leaf.as_der().to_vec(), chain.key().as_der().to_vec(), rest))
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

// ---------- signing ----------

#[derive(Debug, Clone, Deserialize)]
pub struct SignSpec {
    pub identity_id: String,
    #[serde(default)]
    pub password: String,
    pub page: u16,
    /// [x1, y1, x2, y2] in PDF user space (points, origin bottom-left).
    /// Zero-area rect makes an invisible signature.
    pub rect: [f32; 4],
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub contact: Option<String>,
    /// Signing name shown in the appearance; defaults to the certificate CN.
    #[serde(default)]
    pub name: Option<String>,
    /// Lock the document against further changes (DocMDP P=1).
    #[serde(default)]
    pub lock: bool,
}

const SIG_PLACEHOLDER_BYTES: usize = 12288;
/// Ten-digit placeholder; real offsets are padded with spaces to this width.
const BR_PLACEHOLDER: i64 = 9_999_999_999;

fn pdf_string(s: &str) -> Object {
    Object::string_literal(s)
}

fn pdf_date_now() -> String {
    // D:YYYYMMDDHHmmSSZ in UTC, built without chrono.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_unix(secs);
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z")
}

fn civil_from_unix(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32)
}

fn escape_pdf_text(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii() && !c.is_control())
        .collect::<String>()
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Appearance stream for a visible signature: name large, details small.
fn signature_appearance(w: f32, h: f32, name: &str, lines: &[String]) -> Vec<u8> {
    let mut s = String::new();
    // subtle frame + tint
    s.push_str("q 0.93 0.95 0.99 rg 0 0 ");
    s.push_str(&format!("{w:.2} {h:.2} re f Q\n"));
    s.push_str(&format!("q 0.35 0.45 0.70 RG 0.75 w 0.375 0.375 {:.2} {:.2} re S Q\n", w - 0.75, h - 0.75));
    let pad = 4.0f32;
    let big = (h * 0.32).clamp(7.0, 16.0);
    let small = (h * 0.14).clamp(5.0, 8.0);
    let mut y = h - pad - big;
    s.push_str(&format!(
        "BT /Helv {big:.1} Tf 0.10 0.15 0.35 rg {pad:.1} {y:.1} Td ({}) Tj ET\n",
        escape_pdf_text(name)
    ));
    y -= small * 1.5;
    for l in lines {
        if y < pad {
            break;
        }
        s.push_str(&format!(
            "BT /Helv {small:.1} Tf 0.25 0.25 0.30 rg {pad:.1} {y:.1} Td ({}) Tj ET\n",
            escape_pdf_text(l)
        ));
        y -= small * 1.35;
    }
    s.into_bytes()
}

/// Sign `bytes` and return the new file bytes (original bytes + incremental update).
pub fn sign(bytes: &[u8], password: Option<&str>, store: &IdentityStore, spec: &SignSpec) -> Result<Vec<u8>> {
    use pdf_oxide::signatures::SigningCredentials;

    let creds = store.credentials(&spec.identity_id, &spec.password)?;
    let sc = SigningCredentials::from_pem(&creds.cert_pem, &creds.key_pem)
        .map_err(|e| pdf(format!("credentials: {e}")))?;
    let cn = sc
        .subject()
        .map(|s| display_name(&s))
        .unwrap_or_else(|_| "Signer".into());
    let name = spec.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or(cn);

    // Parse the existing file. Encrypted docs must be decrypted for lopdf to
    // append; PDFium already validated the password upstream.
    let prev = load_doc(bytes, password)?;
    let pages = prev.get_pages();
    let page_id = *pages
        .get(&(spec.page as u32 + 1))
        .ok_or_else(|| SheafError::NoSuchPage(spec.page))?;
    let catalog_id = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| pdf("no /Root"))?;

    let mut inc = IncrementalDocument::create_from(bytes.to_vec(), prev);

    // ---- objects ----
    let font_id = inc.new_document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding"
    });
    let [x1, y1, x2, y2] = spec.rect;
    let (w, h) = ((x2 - x1).abs(), (y2 - y1).abs());
    let visible = w > 1.0 && h > 1.0;
    let date = pdf_date_now();
    let mut lines = vec![format!("Digitally signed {}", &date[2..])];
    if let Some(r) = spec.reason.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("Reason: {r}"));
    }
    if let Some(l) = spec.location.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(format!("Location: {l}"));
    }
    let ap_id = {
        let content = if visible {
            signature_appearance(w, h, &name, &lines)
        } else {
            Vec::new()
        };
        let mut st = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
                "Resources" => dictionary! { "Font" => dictionary! { "Helv" => Object::Reference(font_id) } }
            },
            content,
        );
        st = st.with_compression(false);
        inc.new_document.add_object(Object::Stream(st))
    };

    let mut sig = Dictionary::new();
    sig.set("Type", "Sig");
    sig.set("Filter", "Adobe.PPKLite");
    sig.set("SubFilter", "adbe.pkcs7.detached");
    sig.set("Name", pdf_string(&name));
    sig.set("M", pdf_string(&date));
    if let Some(r) = spec.reason.as_deref().filter(|s| !s.trim().is_empty()) {
        sig.set("Reason", pdf_string(r));
    }
    if let Some(l) = spec.location.as_deref().filter(|s| !s.trim().is_empty()) {
        sig.set("Location", pdf_string(l));
    }
    if let Some(c) = spec.contact.as_deref().filter(|s| !s.trim().is_empty()) {
        sig.set("ContactInfo", pdf_string(c));
    }
    // Placeholders, patched in place after serialization. Written last so the
    // hex string and the ByteRange are easy to find and fixed-width.
    // Fixed-width placeholders so the real values can be patched in place.
    sig.set(
        "ByteRange",
        vec![
            Object::Integer(0),
            Object::Integer(BR_PLACEHOLDER),
            Object::Integer(BR_PLACEHOLDER),
            Object::Integer(BR_PLACEHOLDER),
        ],
    );
    sig.set(
        "Contents",
        Object::String(vec![0u8; SIG_PLACEHOLDER_BYTES], lopdf::StringFormat::Hexadecimal),
    );
    if spec.lock {
        sig.set(
            "Reference",
            vec![Object::Dictionary(dictionary! {
                "Type" => "SigRef", "TransformMethod" => "DocMDP",
                "TransformParams" => dictionary! { "Type" => "TransformParams", "P" => 1, "V" => "1.2" }
            })],
        );
    }
    let sig_id = inc.new_document.add_object(sig);

    let field_name = format!("Sheaf-Signature-{}", nanos() % 1_000_000_000);
    let mut widget = dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig",
        "T" => pdf_string(&field_name),
        "Rect" => vec![Object::Real(x1.min(x2)), Object::Real(y1.min(y2)), Object::Real(x1.max(x2)), Object::Real(y1.max(y2))],
        "F" => 132, // Print + Locked
        "P" => Object::Reference(page_id),
        "V" => Object::Reference(sig_id),
        "AP" => dictionary! { "N" => Object::Reference(ap_id) }
    };
    if !visible {
        widget.set("Rect", vec![0.into(), 0.into(), 0.into(), 0.into()]);
    }
    let widget_id = inc.new_document.add_object(widget);

    // ---- page: append widget to Annots ----
    inc.opt_clone_object_to_new_document(page_id)
        .map_err(|e| pdf(format!("clone page: {e}")))?;
    {
        let annots_existing: Option<Vec<Object>> = {
            let page = inc.new_document.get_dictionary(page_id).map_err(|e| pdf(format!("page: {e}")))?;
            match page.get(b"Annots") {
                Ok(Object::Array(a)) => Some(a.clone()),
                Ok(Object::Reference(r)) => {
                    let r = *r;
                    inc.opt_clone_object_to_new_document(r).ok();
                    inc.new_document.get_object(r).ok().and_then(|o| o.as_array().ok().cloned())
                        .or_else(|| inc.get_prev_documents().get_object(r).ok().and_then(|o| o.as_array().ok().cloned()))
                }
                _ => None,
            }
        };
        let mut annots = annots_existing.unwrap_or_default();
        annots.push(Object::Reference(widget_id));
        let page = inc.new_document.get_dictionary_mut(page_id).map_err(|e| pdf(format!("page: {e}")))?;
        page.set("Annots", annots);
    }

    // ---- catalog: AcroForm with the field, SigFlags, DocMDP perms ----
    inc.opt_clone_object_to_new_document(catalog_id)
        .map_err(|e| pdf(format!("clone catalog: {e}")))?;
    {
        let existing_acro: Option<Object> = inc
            .new_document
            .get_dictionary(catalog_id)
            .ok()
            .and_then(|c| c.get(b"AcroForm").ok().cloned());
        let mut acro: Dictionary = match existing_acro {
            Some(Object::Dictionary(d)) => d,
            Some(Object::Reference(r)) => {
                inc.opt_clone_object_to_new_document(r).ok();
                inc.new_document
                    .get_dictionary(r)
                    .ok()
                    .cloned()
                    .or_else(|| inc.get_prev_documents().get_dictionary(r).ok().cloned())
                    .unwrap_or_default()
            }
            _ => Dictionary::new(),
        };
        let mut fields = match acro.get(b"Fields") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(Object::Reference(r)) => inc
                .get_prev_documents()
                .get_object(*r)
                .ok()
                .and_then(|o| o.as_array().ok().cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        fields.push(Object::Reference(widget_id));
        acro.set("Fields", fields);
        let flags = acro.get(b"SigFlags").and_then(Object::as_i64).unwrap_or(0);
        acro.set("SigFlags", flags | 3);
        if !acro.has(b"DR") {
            acro.set("DR", dictionary! { "Font" => dictionary! { "Helv" => Object::Reference(font_id) } });
        }
        let cat = inc.new_document.get_dictionary_mut(catalog_id).map_err(|e| pdf(format!("catalog: {e}")))?;
        cat.set("AcroForm", Object::Dictionary(acro));
        if spec.lock {
            cat.set("Perms", dictionary! { "DocMDP" => Object::Reference(sig_id) });
        }
    }

    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(|e| pdf(format!("write: {e}")))?;

    // ---- locate placeholders (only in the appended region) ----
    // Find the signature object by id, then its /Contents hex token. When
    // the document is encrypted, lopdf wrote the placeholder as ciphertext
    // (a different length); the spec requires Contents to be stored raw, so
    // we overwrite that token with plaintext hex regardless.
    let base = bytes.len();
    let obj_head = format!("\n{} {} obj", sig_id.0, sig_id.1);
    let o_start = base
        + find(&out[base..], obj_head.as_bytes())
            .ok_or_else(|| pdf("signature object not found"))?;
    let o_end = o_start + find(&out[o_start..], b"endobj").ok_or_else(|| pdf("signature object end"))?;
    let c_key = o_start + find(&out[o_start..o_end], b"/Contents").ok_or_else(|| pdf("Contents not found"))?;
    let c_start = c_key + find(&out[c_key..o_end], b"<").ok_or_else(|| pdf("Contents string"))?;
    let c_end = c_start + find(&out[c_start..o_end], b">").ok_or_else(|| pdf("Contents string"))? + 1;
    let br_key = o_start + find(&out[o_start..o_end], b"/ByteRange").ok_or_else(|| pdf("ByteRange not found"))?;
    let br_open = br_key + find(&out[br_key..o_end], b"[").ok_or_else(|| pdf("ByteRange array"))?;
    let br_close = br_open + find(&out[br_open..o_end], b"]").ok_or_else(|| pdf("ByteRange array"))?;
    let br_text = format!("[{} {} {} {}]", 0, c_start, c_end, out.len() - c_end);
    let avail = br_close + 1 - br_open;
    if br_text.len() > avail {
        return Err(pdf("ByteRange does not fit its placeholder"));
    }
    let padded = format!("{:<width$}", br_text, width = avail);
    out[br_open..br_close + 1].copy_from_slice(padded.as_bytes());

    fill_signature(&mut out, c_start, c_end, &sc, spec)?;
    Ok(out)
}

fn fill_signature(
    out: &mut [u8],
    c_start: usize,
    c_end: usize,
    sc: &pdf_oxide::signatures::SigningCredentials,
    spec: &SignSpec,
) -> Result<()> {
    use pdf_oxide::signatures::{PdfSigner, SignOptions};
    let mut signed = Vec::with_capacity(out.len());
    signed.extend_from_slice(&out[..c_start]);
    signed.extend_from_slice(&out[c_end..]);
    let opts = SignOptions {
        reason: spec.reason.clone(),
        location: spec.location.clone(),
        contact_info: spec.contact.clone(),
        ..Default::default()
    };
    let signer = PdfSigner::new(sc.clone(), opts);
    let cms = signer.sign(&signed).map_err(|e| pdf(format!("sign: {e}")))?;
    let slot = &mut out[c_start + 1..c_end - 1];
    let hex: String = cms.iter().map(|b| format!("{b:02X}")).collect();
    if hex.len() > slot.len() {
        return Err(pdf("signature larger than the reserved space"));
    }
    slot[..hex.len()].copy_from_slice(hex.as_bytes());
    for b in &mut slot[hex.len()..] {
        *b = b'0';
    }
    Ok(())
}


/// Parse with lopdf, decrypting when needed. Encrypted documents must be
/// loaded with the password up front: `load_mem` + `decrypt` leaves the
/// object table empty for object-stream files.
fn load_doc(bytes: &[u8], password: Option<&str>) -> Result<Document> {
    let probe = Document::load_mem(bytes).map_err(|e| pdf(format!("parse: {e}")))?;
    if !probe.is_encrypted() {
        return Ok(probe);
    }
    drop(probe);
    Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions::with_password(password.unwrap_or("")),
    )
    .map_err(|e| {
        let m = e.to_string();
        if m.to_lowercase().contains("password") || m.to_lowercase().contains("decrypt") {
            SheafError::PasswordRequired
        } else {
            pdf(format!("parse: {m}"))
        }
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// ---------- verifying ----------

#[derive(Debug, Clone, Serialize)]
pub struct SignatureInfo {
    pub field_name: String,
    pub signer: String,
    pub subject: String,
    pub issuer: String,
    pub self_signed: bool,
    pub signed_at: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub page: Option<u16>,
    pub rect: Option<[f32; 4]>,
    /// "valid" | "modified" | "invalid" | "unknown"
    pub status: String,
    pub covers_whole_document: bool,
    pub locks_document: bool,
    pub messages: Vec<String>,
}

pub fn list_signatures(bytes: &[u8], password: Option<&str>) -> Result<Vec<SignatureInfo>> {
    use pdf_oxide::signatures::{extract_signer_certificate_der, verify_signer_detached, SignerVerify};

    let doc = load_doc(bytes, password)?;
    let pages = doc.get_pages();
    let page_of = |id: ObjectId| pages.iter().find(|(_, &p)| p == id).map(|(n, _)| (*n - 1) as u16);
    let mut out = Vec::new();

    let catalog = doc.catalog().map_err(|e| pdf(format!("catalog: {e}")))?;
    let Ok(acro) = catalog.get(b"AcroForm") else {
        return Ok(out);
    };
    let acro = resolve_dict(&doc, acro).unwrap_or_default();
    let fields = match acro.get(b"Fields") {
        Ok(o) => resolve_array(&doc, o).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let docmdp = catalog
        .get(b"Perms")
        .ok()
        .and_then(|p| resolve_dict(&doc, p))
        .and_then(|p| p.get(b"DocMDP").ok().and_then(|o| o.as_reference().ok()));

    let mut stack: Vec<Object> = fields;
    let mut seen = std::collections::HashSet::new();
    while let Some(f) = stack.pop() {
        let Ok(fid) = f.as_reference() else { continue };
        if !seen.insert(fid) {
            continue;
        }
        let Ok(fd) = doc.get_dictionary(fid) else { continue };
        if let Ok(kids) = fd.get(b"Kids").and_then(|k| Ok(resolve_array(&doc, k).unwrap_or_default())) {
            stack.extend(kids);
        }
        let ft = fd.get(b"FT").ok().and_then(|o| o.as_name().ok()).map(|n| n.to_vec());
        if ft.as_deref() != Some(b"Sig") {
            continue;
        }
        let Ok(v) = fd.get(b"V") else { continue };
        let Some(sig) = resolve_dict(&doc, v) else { continue };
        let vref = v.as_reference().ok();

        let get_str = |k: &[u8]| {
            sig.get(k)
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned())
        };
        let field_name = fd
            .get(b"T")
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        let br: Vec<i64> = sig
            .get(b"ByteRange")
            .ok()
            .and_then(|o| resolve_array(&doc, o))
            .map(|a| a.iter().filter_map(|x| x.as_i64().ok()).collect())
            .unwrap_or_default();
        // Contents must be read from the raw file (the gap between the two
        // byte ranges), never through the parser: in encrypted documents the
        // parser would AES-"decrypt" what is stored as plaintext hex.
        let contents: Vec<u8> = if br.len() == 4 {
            let gap_start = (br[0] + br[1]) as usize;
            let gap_end = br[2] as usize;
            if gap_start < gap_end && gap_end <= bytes.len() {
                let raw = &bytes[gap_start..gap_end];
                let inner = raw
                    .iter()
                    .position(|&b| b == b'<')
                    .map(|i| &raw[i + 1..])
                    .unwrap_or(raw);
                let inner = inner
                    .iter()
                    .position(|&b| b == b'>')
                    .map(|i| &inner[..i])
                    .unwrap_or(inner);
                hex_decode(inner)
            } else {
                Vec::new()
            }
        } else {
            sig.get(b"Contents")
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(|b| b.to_vec())
                .unwrap_or_default()
        };
        let rect = fd.get(b"Rect").ok().and_then(|o| o.as_array().ok()).and_then(|a| {
            if a.len() == 4 {
                let f = |o: &Object| o.as_float().ok().or_else(|| o.as_i64().ok().map(|i| i as f32));
                Some([f(&a[0])?, f(&a[1])?, f(&a[2])?, f(&a[3])?])
            } else {
                None
            }
        });
        let page = fd
            .get(b"P")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .and_then(page_of);

        let mut messages = Vec::new();
        let mut status = "unknown".to_string();
        let mut covers = false;
        let mut subject = String::new();
        let mut issuer = String::new();
        let mut self_signed = false;

        // Strip trailing zero padding from the hex contents.
        let cms_end = contents.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
        let cms = &contents[..cms_end];

        if br.len() == 4 && !cms.is_empty() {
            let (a, b, c, d) = (br[0] as usize, br[1] as usize, br[2] as usize, br[3] as usize);
            if a + b <= bytes.len() && c + d <= bytes.len() && a + b <= c {
                covers = a == 0 && c + d == bytes.len();
                let mut signed = Vec::with_capacity(b + d);
                signed.extend_from_slice(&bytes[a..a + b]);
                signed.extend_from_slice(&bytes[c..c + d]);
                match verify_signer_detached(cms, &signed) {
                    Ok(SignerVerify::Valid) => {
                        status = if covers { "valid".into() } else { "modified".into() };
                        if !covers {
                            messages.push("The document was changed after this signature was applied.".into());
                        }
                    }
                    Ok(SignerVerify::Invalid) => {
                        status = "invalid".into();
                        messages.push("The signed bytes do not match the signature.".into());
                    }
                    Ok(SignerVerify::Unknown) => {
                        messages.push("Signature algorithm is not supported for verification.".into());
                    }
                    Err(e) => messages.push(format!("Could not parse signature: {e}")),
                }
                if let Ok(der) = extract_signer_certificate_der(cms) {
                    if let Ok((s, i, ss, _)) = describe_cert(&der) {
                        subject = s;
                        issuer = i;
                        self_signed = ss;
                    }
                }
            } else {
                status = "invalid".into();
                messages.push("ByteRange points outside the file.".into());
            }
        } else if cms.is_empty() {
            messages.push("Unsigned signature field.".into());
        }
        if self_signed && status == "valid" {
            messages.push("Signed with a self-signed certificate; identity is not vouched for by a certificate authority.".into());
        }

        let locks = spec_locks(&sig) || (docmdp.is_some() && docmdp == vref);
        out.push(SignatureInfo {
            field_name,
            signer: get_str(b"Name").unwrap_or_else(|| display_name(&subject)),
            subject,
            issuer,
            self_signed,
            signed_at: get_str(b"M"),
            reason: get_str(b"Reason"),
            location: get_str(b"Location"),
            page,
            rect,
            status,
            covers_whole_document: covers,
            locks_document: locks,
            messages,
        });
    }
    Ok(out)
}

fn hex_decode(h: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = h
        .iter()
        .filter(|b| b.is_ascii_hexdigit())
        .map(|b| (*b as char).to_digit(16).unwrap() as u8)
        .collect();
    digits.chunks(2).map(|c| (c[0] << 4) | c.get(1).copied().unwrap_or(0)).collect()
}

fn spec_locks(sig: &Dictionary) -> bool {
    sig.get(b"Reference")
        .ok()
        .and_then(|r| r.as_array().ok())
        .map(|a| {
            a.iter().any(|r| {
                r.as_dict()
                    .ok()
                    .and_then(|d| d.get(b"TransformMethod").ok())
                    .and_then(|m| m.as_name().ok())
                    == Some(b"DocMDP")
            })
        })
        .unwrap_or(false)
}

fn resolve_dict(doc: &Document, o: &Object) -> Option<Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(r) => doc.get_dictionary(*r).ok().cloned(),
        _ => None,
    }
}

fn resolve_array(doc: &Document, o: &Object) -> Option<Vec<Object>> {
    match o {
        Object::Array(a) => Some(a.clone()),
        Object::Reference(r) => doc.get_object(*r).ok().and_then(|x| x.as_array().ok().cloned()),
        _ => None,
    }
}

// ---------- protecting ----------

#[derive(Debug, Clone, Deserialize)]
pub struct SecuritySpec {
    /// Password needed to open. Empty means anyone can open.
    #[serde(default)]
    pub user_password: String,
    /// Password to change permissions. Empty defaults to the user password,
    /// or a random one if both are empty.
    #[serde(default)]
    pub owner_password: String,
    #[serde(default = "t")]
    pub allow_print: bool,
    #[serde(default = "t")]
    pub allow_print_high_quality: bool,
    #[serde(default = "t")]
    pub allow_modify: bool,
    #[serde(default = "t")]
    pub allow_copy: bool,
    #[serde(default = "t")]
    pub allow_annotate: bool,
    #[serde(default = "t")]
    pub allow_fill_forms: bool,
    #[serde(default = "t")]
    pub allow_assemble: bool,
    #[serde(default = "t")]
    pub allow_accessibility: bool,
}
fn t() -> bool {
    true
}

/// Returns AES-256 encrypted bytes. The result is a full rewrite (not
/// incremental), which also drops any prior signatures; callers should warn.
pub fn protect(bytes: &[u8], current_password: Option<&str>, spec: &SecuritySpec) -> Result<Vec<u8>> {
    use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};

    let mut doc = load_doc(bytes, current_password)?;
    doc.encryption_state = None;
    let mut perms = Permissions::empty();
    if spec.allow_print {
        perms |= Permissions::PRINTABLE;
    }
    if spec.allow_print_high_quality {
        perms |= Permissions::PRINTABLE_IN_HIGH_QUALITY;
    }
    if spec.allow_modify {
        perms |= Permissions::MODIFIABLE;
    }
    if spec.allow_copy {
        perms |= Permissions::COPYABLE;
    }
    if spec.allow_annotate {
        perms |= Permissions::ANNOTABLE;
    }
    if spec.allow_fill_forms {
        perms |= Permissions::FILLABLE;
    }
    if spec.allow_assemble {
        perms |= Permissions::ASSEMBLABLE;
    }
    if spec.allow_accessibility {
        perms |= Permissions::COPYABLE_FOR_ACCESSIBILITY;
    }
    let owner = if !spec.owner_password.is_empty() {
        spec.owner_password.clone()
    } else if !spec.user_password.is_empty() {
        spec.user_password.clone()
    } else {
        format!("{:032x}", nanos())
    };
    let mut key = [0u8; 32];
    {
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut key);
    }
    let cf: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let state = EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), cf)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: &owner,
        user_password: &spec.user_password,
        permissions: perms,
    })
    .map_err(|e| pdf(format!("encryption: {e}")))?;
    doc.encrypt(&state).map_err(|e| pdf(format!("encrypt: {e}")))?;
    // AES-256 R6 needs PDF 1.7 ext level 8 / 2.0; readers accept 1.7.
    if doc.version.as_str() < "1.7" {
        doc.version = "1.7".into();
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| pdf(format!("write: {e}")))?;
    Ok(out)
}

/// Remove encryption entirely (requires the current password to have opened it).
pub fn unprotect(bytes: &[u8], current_password: Option<&str>) -> Result<Vec<u8>> {
    let probe = Document::load_mem(bytes).map_err(|e| pdf(format!("parse: {e}")))?;
    if !probe.is_encrypted() {
        return Ok(bytes.to_vec());
    }
    drop(probe);
    let mut doc = load_doc(bytes, current_password)?;
    doc.encryption_state = None;
    doc.trailer.remove(b"Encrypt");
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| pdf(format!("write: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(1_788_471_200), (2026, 9, 3, 21, 33, 20));
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("O=Sheaf, CN=Brian"), "Brian");
        assert_eq!(display_name("CN=A\\, B"), "A\\");
    }
}
