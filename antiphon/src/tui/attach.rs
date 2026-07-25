use std::path::{Path, PathBuf};

use antiphon_render::AttachmentPart;

/// One file attached to the compose, read fully when added so
/// the review screen never promises bytes it cannot deliver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Attachment {
    pub path: PathBuf,
    pub filename: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

impl Attachment {
    pub fn as_part(&self) -> AttachmentPart<'_> {
        AttachmentPart {
            filename: &self.filename,
            content_type: self.content_type,
            bytes: &self.bytes,
        }
    }

    pub fn label(&self) -> String {
        format!(
            "{} ({}, {} bytes)",
            self.filename,
            self.content_type,
            self.bytes.len()
        )
    }
}

/// Loads the file behind a prompt answer: ~ expands to $HOME,
/// and any failure names the actual path so the prompt can
/// re-ask.
pub(super) fn load(input: &str) -> Result<Attachment, String> {
    let path = expand_tilde(input.trim());
    let bytes = std::fs::read(&path).map_err(|error| {
        format!("attachment {}: {error}", path.display())
    })?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            format!("attachment {}: no file name", path.display())
        })?;
    Ok(Attachment {
        content_type: antiphon_render::content_type_for(&filename),
        path,
        filename,
        bytes,
    })
}

pub(super) fn expand_tilde(input: &str) -> PathBuf {
    let Some(rest) = input.strip_prefix("~/") else {
        return PathBuf::from(input);
    };
    let Some(home) = std::env::var_os("HOME") else {
        return PathBuf::from(input);
    };
    Path::new(&home).join(rest)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::TempDir;
    use super::*;

    #[test]
    fn loading_reads_bytes_and_infers_the_type() {
        let dir = TempDir::new();
        let path = dir.path.join("scan.pdf");
        std::fs::write(&path, b"%PDF-1.7 fake").unwrap();
        let attachment = load(path.to_str().unwrap()).unwrap();
        assert_eq!(attachment.filename, "scan.pdf");
        assert_eq!(attachment.content_type, "application/pdf");
        assert_eq!(attachment.bytes, b"%PDF-1.7 fake");
        assert!(attachment.label().contains("13 bytes"));
    }

    #[test]
    fn a_missing_file_errors_naming_the_path() {
        let error = load("/nonexistent/report.pdf").unwrap_err();
        assert!(
            error.starts_with("attachment /nonexistent/report.pdf:"),
            "{error}"
        );
    }

    #[test]
    fn tilde_expands_against_home() {
        let expanded = expand_tilde("~/mail/a.txt");
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expanded, Path::new(&home).join("mail/a.txt"));
        assert_eq!(
            expand_tilde("/absolute/a.txt"),
            Path::new("/absolute/a.txt")
        );
    }
}
