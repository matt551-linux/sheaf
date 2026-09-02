use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum SheafError {
    #[error("engine error: {0}")]
    Engine(String),
    #[error("pdf error: {0}")]
    Pdf(String),
    #[error("document is password protected")]
    PasswordRequired,
    #[error("no such document: {0}")]
    NoSuchDocument(u32),
    #[error("no such page: {0}")]
    NoSuchPage(u16),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SheafError>;

/// Shape sent to the frontend so it can branch on `kind` (for example to
/// prompt for a password) while still showing `message`.
#[derive(Serialize)]
struct ErrorPayload {
    kind: &'static str,
    message: String,
}

impl Serialize for SheafError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let kind = match self {
            SheafError::Engine(_) => "engine",
            SheafError::Pdf(_) => "pdf",
            SheafError::PasswordRequired => "password_required",
            SheafError::NoSuchDocument(_) => "no_such_document",
            SheafError::NoSuchPage(_) => "no_such_page",
            SheafError::Io(_) => "io",
        };
        ErrorPayload {
            kind,
            message: self.to_string(),
        }
        .serialize(s)
    }
}
