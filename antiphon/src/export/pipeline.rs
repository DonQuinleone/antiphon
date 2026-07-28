use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;

use super::ExportError;

/// Write the encrypted archive and return the number of files
/// archived and the encrypted bytes on disk. The data streams
/// tar -> gzip -> age -> file, so memory use stays flat however
/// large the mailbox is.
pub(super) fn write_archive(
    maildir: &Path,
    account: &str,
    destination: &Path,
    recipients: &[Box<dyn age::Recipient + Send>],
) -> Result<(u64, u64), ExportError> {
    let output = |err: io::Error| ExportError::Output {
        path: destination.to_path_buf(),
        message: err.to_string(),
    };
    let file = File::create(destination).map_err(output)?;
    let counter = CountingWriter::new(BufWriter::new(file));
    let encryptor = age::Encryptor::with_recipients(
        recipients.iter().map(|key| &**key as &dyn age::Recipient),
    )
    .map_err(|err| ExportError::Encrypt(err.to_string()))?;
    let stream = encryptor.wrap_output(counter).map_err(output)?;
    let gz = GzEncoder::new(stream, Compression::default());
    let mut builder = tar::Builder::new(gz);
    let files = append_tree(&mut builder, maildir, Path::new(account))?;
    let gz = builder.into_inner().map_err(output)?;
    let stream = gz.finish().map_err(output)?;
    let mut counter = stream.finish().map_err(output)?;
    counter.flush().map_err(output)?;
    Ok((files, counter.written))
}

/// Recursively append `dir` under `name`, returning how many
/// regular files went in. Entries are sorted so identical trees
/// archive identically.
fn append_tree(
    builder: &mut tar::Builder<impl Write>,
    dir: &Path,
    name: &Path,
) -> Result<u64, ExportError> {
    let archive = |err: io::Error| ExportError::Archive {
        path: dir.to_path_buf(),
        message: err.to_string(),
    };
    builder.append_dir(name, dir).map_err(archive)?;
    let entries = std::fs::read_dir(dir).map_err(archive)?;
    let mut entries: Vec<_> =
        entries.collect::<io::Result<_>>().map_err(archive)?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut files = 0;
    for entry in entries {
        let path = entry.path();
        let child = name.join(entry.file_name());
        if path.is_dir() {
            files += append_tree(builder, &path, &child)?;
            continue;
        }
        builder
            .append_path_with_name(&path, &child)
            .map_err(archive)?;
        files += 1;
    }
    Ok(files)
}

struct CountingWriter<W> {
    inner: W,
    written: u64,
}

impl<W> CountingWriter<W> {
    fn new(inner: W) -> CountingWriter<W> {
        CountingWriter { inner, written: 0 }
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(buf)?;
        self.written += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
