use crate::error::{Error, Result, EXIT_MANIFEST};
use crate::http;
use crate::util;
use crate::Log;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    pub url: String,
    pub file_offset: u64,
    pub file_size: u64,
    pub hash_value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub original_file_size: u64,
    #[serde(default)]
    pub package_digest: Option<String>,
    #[serde(default)]
    pub number_of_split_files: Option<u64>,
    pub pieces: Vec<Piece>,
    #[serde(default)]
    pub playgo_chunk_crc_hash_value: Option<String>,
    #[serde(default)]
    pub playgo_chunk_crc_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Source {
    Url(String),
    File(PathBuf),
}

impl Source {
    pub fn describe(&self) -> String {
        match self {
            Source::Url(url) => url.clone(),
            Source::File(path) => path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedManifest {
    pub manifest: Manifest,
    pub source: Source,
    pub base_name: String,
    pub raw: String,
}

pub fn load(input: &str, log: &Log) -> Result<LoadedManifest> {
    if util::looks_like_url(input) {
        let url = util::crc_to_json_url(input);
        if url != input {
            log.info(format!("Convirtiendo .crc en .json: {url}"));
        }
        log.info(format!("Descargando manifiesto: {url}"));
        let bytes = http::get_bytes(&url, MAX_MANIFEST_BYTES, EXIT_MANIFEST)?;
        let raw = String::from_utf8(bytes)
            .map_err(|e| Error::manifest(format!("el manifiesto no es UTF-8 válido: {e}")))?;
        let base_name = util::url_basename(&url)
            .map(|name| util::strip_json_ext(&name))
            .ok_or_else(|| {
                Error::manifest(format!("no se pudo obtener un nombre de archivo de {url}"))
            })?;
        return finish(raw, Source::Url(url), base_name, log);
    }

    let mut path = PathBuf::from(input);
    if path.is_dir() {
        path = find_in_dir(&path)?;
    }

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "crc" {
        let json_path = path.with_extension("json");
        if !json_path.exists() {
            return Err(Error::manifest(format!(
                "'{}' es un .crc, pero no existe '{}'. Descarga el manifiesto .json (cambiando .crc por .json en la URL) y vuelve a intentarlo.",
                path.display(),
                json_path.display()
            )));
        }
        log.info(format!("Usando manifiesto JSON: {}", json_path.display()));
        path = json_path;
    }

    if !path.exists() {
        return Err(Error::manifest(format!(
            "no existe el manifiesto '{}'",
            path.display()
        )));
    }
    if path.is_dir() {
        return load(&path.to_string_lossy(), log);
    }

    log.info(format!("Leyendo manifiesto: {}", path.display()));
    let raw = fs::read_to_string(&path)
        .map_err(|e| Error::io_path("no se pudo leer el manifiesto", &path, e))?;
    let base_name = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    finish(raw, Source::File(path), base_name, log)
}

fn finish(raw: String, source: Source, base_name: String, log: &Log) -> Result<LoadedManifest> {
    let raw = raw.trim_start_matches('\u{feff}').to_string();
    let manifest: Manifest = serde_json::from_str(&raw)
        .map_err(|e| Error::manifest(format!("manifiesto JSON inválido: {e}")))?;
    let manifest = manifest.validate()?;
    let base_name = util::strip_json_ext(&base_name);
    if !util::is_valid_output_stem(&base_name) {
        return Err(Error::manifest(format!(
            "el nombre de salida derivado del manifiesto no es válido: '{base_name}'"
        )));
    }
    log.detail(format!(
        "Manifiesto: {} piezas, {} bytes, packageDigest {}",
        manifest.pieces.len(),
        manifest.original_file_size,
        manifest
            .package_digest
            .as_deref()
            .unwrap_or("(no indicado)")
    ));
    if let Some(url) = &manifest.playgo_chunk_crc_url {
        log.detail(format!("playgoChunkCrcUrl: {url}"));
    }
    if let Some(hash) = &manifest.playgo_chunk_crc_hash_value {
        log.detail(format!("playgoChunkCrcHashValue: {hash}"));
    }
    Ok(LoadedManifest {
        manifest,
        source,
        base_name,
        raw,
    })
}

pub fn find_in_dir(dir: &Path) -> Result<PathBuf> {
    let mut parsed = Vec::new();
    let mut crc_files = Vec::new();

    for entry in
        fs::read_dir(dir).map_err(|e| Error::io_path("no se pudo listar la carpeta", dir, e))?
    {
        let entry = entry.map_err(|e| Error::io_path("no se pudo leer una entrada de", dir, e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "crc" {
            crc_files.push(path);
            continue;
        }
        if ext != "json" {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            let text = text.trim_start_matches('\u{feff}');
            if let Ok(manifest) = serde_json::from_str::<Manifest>(text) {
                if !manifest.pieces.is_empty() {
                    parsed.push(path);
                }
            }
        }
    }

    if parsed.is_empty() {
        let extra = if crc_files.is_empty() {
            String::new()
        } else {
            format!(
                " Hay {} archivo(s) .crc, pero necesito el .json (cambia .crc por .json en la URL).",
                crc_files.len()
            )
        };
        return Err(Error::manifest(format!(
            "no encontré ningún manifiesto .json válido en '{}'.{extra}",
            dir.display()
        )));
    }

    let preferred: Vec<PathBuf> = parsed
        .iter()
        .filter(|path| {
            !path
                .file_name()
                .map(|name| {
                    name.to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".manifest.json")
                })
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    if preferred.len() == 1 {
        return Ok(preferred.into_iter().next().unwrap());
    }
    if preferred.is_empty() && parsed.len() == 1 {
        return Ok(parsed.into_iter().next().unwrap());
    }

    let names = parsed
        .iter()
        .map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(Error::manifest(format!(
        "hay varios manifiestos en '{}': {names}. Usa --manifest para elegir uno.",
        dir.display()
    )))
}

impl Manifest {
    pub fn validate(mut self) -> Result<Self> {
        if self.pieces.is_empty() {
            return Err(Error::manifest("el manifiesto no contiene piezas"));
        }
        if self.package_digest.as_deref() == Some("") {
            self.package_digest = None;
        }
        if let Some(count) = self.number_of_split_files {
            if count as usize != self.pieces.len() {
                return Err(Error::manifest(format!(
                    "numberOfSplitFiles dice {count}, pero el manifiesto tiene {} piezas",
                    self.pieces.len()
                )));
            }
        }

        self.pieces.sort_by_key(|piece| piece.file_offset);

        let mut expected = 0u64;
        for (index, piece) in self.pieces.iter().enumerate() {
            if piece.file_offset != expected {
                return Err(Error::manifest(format!(
                    "hueco o solape en la pieza {}: fileOffset {} (se esperaba {})",
                    index + 1,
                    piece.file_offset,
                    expected
                )));
            }
            if piece.file_size == 0 {
                return Err(Error::manifest(format!(
                    "la pieza {} tiene fileSize 0",
                    index + 1
                )));
            }
            if !util::is_hex64(&piece.hash_value) {
                return Err(Error::manifest(format!(
                    "la pieza {} tiene un hashValue inválido (deben ser 64 caracteres hex): {}",
                    index + 1,
                    piece.hash_value
                )));
            }
            match util::url_basename(&piece.url) {
                Some(name) if util::is_valid_piece_filename(&name) => {}
                _ => {
                    return Err(Error::manifest(format!(
                        "la pieza {} tiene una url sin nombre de archivo válido: {}",
                        index + 1,
                        piece.url
                    )))
                }
            }
            expected = expected
                .checked_add(piece.file_size)
                .ok_or_else(|| Error::manifest("el tamaño total del manifiesto desborda u64"))?;
        }

        if expected != self.original_file_size {
            return Err(Error::manifest(format!(
                "la suma de las piezas es {expected} bytes, pero originalFileSize es {}",
                self.original_file_size
            )));
        }
        if let Some(digest) = self.package_digest.as_deref() {
            if !util::is_hex64(digest) {
                return Err(Error::manifest(format!(
                    "packageDigest inválido (deben ser 64 caracteres hex): {digest}"
                )));
            }
        }
        Ok(self)
    }
}
