use crate::error::{Error, Result, EXIT_IO};
use crate::util;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    pub version: u32,
    pub mode: String,
    pub original_file_size: u64,
    #[serde(default)]
    pub package_digest: Option<String>,
    pub completed_pieces: usize,
    pub bytes_written: u64,
    pub updated_unix: u64,
}

impl Journal {
    pub fn new(
        mode: &str,
        original_file_size: u64,
        package_digest: Option<String>,
        completed_pieces: usize,
        bytes_written: u64,
    ) -> Self {
        Self {
            version: 1,
            mode: mode.to_string(),
            original_file_size,
            package_digest,
            completed_pieces,
            bytes_written,
            updated_unix: now_unix(),
        }
    }

    pub fn touch(&mut self, completed_pieces: usize, bytes_written: u64) {
        self.completed_pieces = completed_pieces;
        self.bytes_written = bytes_written;
        self.updated_unix = now_unix();
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load(path: &Path) -> Result<Option<Journal>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)
        .map_err(|e| Error::io_path("no se pudo leer el journal", path, e))?;
    let journal = serde_json::from_str(&text).map_err(|e| {
        Error::new(
            EXIT_IO,
            format!("journal inválido '{}': {e}", path.display()),
        )
    })?;
    Ok(Some(journal))
}

pub fn save(path: &Path, journal: &Journal) -> Result<()> {
    let text = serde_json::to_string_pretty(journal)
        .map_err(|e| Error::new(EXIT_IO, format!("no se pudo serializar el journal: {e}")))?;
    let tmp = util::with_suffix(path, ".tmp");
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| Error::io_path("no se pudo crear el journal temporal", &tmp, e))?;
        file.write_all(text.as_bytes())
            .map_err(|e| Error::io_path("no se pudo escribir el journal temporal", &tmp, e))?;
        file.write_all(b"\n")
            .map_err(|e| Error::io_path("no se pudo escribir el journal temporal", &tmp, e))?;
        file.sync_all()
            .map_err(|e| Error::io_path("no se pudo sincronizar el journal temporal", &tmp, e))?;
    }
    if let Err(first) = fs::rename(&tmp, path) {
        // En algunos sistemas no se puede reemplazar directamente; reintentamos.
        let _ = fs::remove_file(path);
        fs::rename(&tmp, path).map_err(|second| {
            Error::new(
                EXIT_IO,
                format!(
                    "no se pudo guardar el journal '{}': {first}; reintento: {second}",
                    path.display()
                ),
            )
        })?;
    }
    Ok(())
}

pub fn remove(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|e| Error::io_path("no se pudo borrar el journal", path, e))?;
    }
    Ok(())
}
