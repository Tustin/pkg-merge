use crate::error::{Error, Result, EXIT_IO};
use crate::manifest::Piece;
use crate::progress;
use crate::util;
use crate::Log;
use indicatif::ProgressBar;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum PieceSource {
    Local(PathBuf),
    Download,
}

#[derive(Debug, Clone)]
pub struct LocatedPiece {
    pub index: usize,
    pub piece: Piece,
    pub source: PieceSource,
}

pub struct LocateOptions<'a> {
    pub dirs: &'a [PathBuf],
    pub exclude: &'a [PathBuf],
    pub download_missing: bool,
    pub no_progress: bool,
}

pub fn locate_all(
    pieces: &[Piece],
    options: &LocateOptions,
    log: &Log,
) -> Result<Vec<LocatedPiece>> {
    let candidates = collect_candidates(options.dirs, options.exclude, log)?;
    log.detail(format!("archivos .pkg candidatos: {}", candidates.len()));

    let path_key = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let mut resolved: Vec<Option<PieceSource>> = vec![None; pieces.len()];
    let mut notes: HashMap<usize, String> = HashMap::new();

    // 1) Nombre de archivo tal como aparece en la URL.
    let mut by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in &candidates {
        if let Some(name) = path.file_name() {
            by_name
                .entry(name.to_string_lossy().to_ascii_lowercase())
                .or_default()
                .push(path.clone());
        }
    }
    for (index, piece) in pieces.iter().enumerate() {
        let Some(name) = util::url_basename(&piece.url) else {
            continue;
        };
        let Some(paths) = by_name.get(&name.to_ascii_lowercase()) else {
            continue;
        };
        let available: Vec<&PathBuf> = paths
            .iter()
            .filter(|path| !claimed.contains(&path_key(path)))
            .collect();
        if available.len() == 1 {
            let path = available[0];
            match fs::metadata(path) {
                Ok(meta) if meta.len() == piece.file_size => {
                    claimed.insert(path_key(path));
                    resolved[index] = Some(PieceSource::Local(path.clone()));
                }
                Ok(meta) => {
                    notes.insert(
                        index,
                        format!(
                            "'{}' tiene {} bytes (se esperaban {})",
                            path.display(),
                            meta.len(),
                            piece.file_size
                        ),
                    );
                }
                Err(error) => {
                    notes.insert(
                        index,
                        format!("no se pudo leer '{}': {error}", path.display()),
                    );
                }
            }
        } else if available.len() > 1 {
            notes.insert(
                index,
                format!(
                    "hay {} archivos llamados '{}'; se resolverá por tamaño/hash",
                    available.len(),
                    name
                ),
            );
        }
    }

    // 2) Tamaño único.
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for path in &candidates {
        if claimed.contains(&path_key(path)) {
            continue;
        }
        if let Ok(meta) = fs::metadata(path) {
            by_size.entry(meta.len()).or_default().push(path.clone());
        }
    }
    for (index, piece) in pieces.iter().enumerate() {
        if resolved[index].is_some() {
            continue;
        }
        let Some(paths) = by_size.get(&piece.file_size) else {
            continue;
        };
        let available: Vec<&PathBuf> = paths
            .iter()
            .filter(|path| !claimed.contains(&path_key(path)))
            .collect();
        if available.len() == 1 {
            let path = available[0];
            claimed.insert(path_key(path));
            resolved[index] = Some(PieceSource::Local(path.clone()));
            notes.remove(&index);
        }
    }

    // 3) Hash: último recurso, y el más fiable si el usuario renombró las piezas.
    let unresolved: Vec<usize> = (0..pieces.len())
        .filter(|index| resolved[*index].is_none())
        .collect();
    if !unresolved.is_empty() {
        let remaining: Vec<PathBuf> = candidates
            .iter()
            .filter(|path| !claimed.contains(&path_key(path)))
            .cloned()
            .collect();
        if !remaining.is_empty() {
            let total: u64 = remaining
                .iter()
                .filter_map(|path| fs::metadata(path).ok().map(|meta| meta.len()))
                .sum();
            log.warn(format!(
                "No pude emparejar {} pieza(s) por nombre ni tamaño. Hasheando {} archivo(s) ({})...",
                unresolved.len(),
                remaining.len(),
                util::format_bytes(total)
            ));
            let bar = progress::make_bar(total, !options.no_progress, 0, "hasheando archivos");
            let mut hashes: Vec<(PathBuf, Option<String>)> = Vec::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            for path in &remaining {
                match hash_file_with_bar(path, &mut buffer, &bar) {
                    Ok(hash) => hashes.push((path.clone(), Some(hash))),
                    Err(error) => {
                        log.warn(format!("no se pudo hashear '{}': {error}", path.display()));
                        hashes.push((path.clone(), None));
                    }
                }
            }
            bar.finish_and_clear();

            let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for (path, hash) in hashes {
                if let Some(hash) = hash {
                    by_hash
                        .entry(hash.to_ascii_lowercase())
                        .or_default()
                        .push(path);
                }
            }
            for &index in &unresolved {
                let expected = pieces[index].hash_value.to_ascii_lowercase();
                let Some(paths) = by_hash.get(&expected) else {
                    continue;
                };
                if let Some(path) = paths.iter().find(|path| !claimed.contains(&path_key(path))) {
                    claimed.insert(path_key(path));
                    resolved[index] = Some(PieceSource::Local(path.clone()));
                    notes.remove(&index);
                    if paths.len() > 1 {
                        log.warn(format!(
                            "hay varios archivos idénticos para la pieza {}; usando '{}'",
                            index + 1,
                            path.display()
                        ));
                    }
                }
            }
        }
    }

    // 4) Faltantes.
    let mut missing = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if resolved[index].is_some() {
            continue;
        }
        if options.download_missing {
            resolved[index] = Some(PieceSource::Download);
            log.warn(format!(
                "Pieza {} no encontrada; se descargará desde {}",
                index + 1,
                piece.url
            ));
        } else {
            missing.push(index);
        }
    }
    if !missing.is_empty() {
        let mut message = String::from("faltan piezas o no coinciden:\n");
        for &index in missing.iter().take(20) {
            let piece = &pieces[index];
            let name = util::url_basename(&piece.url).unwrap_or_else(|| piece.url.clone());
            message.push_str(&format!(
                "  - pieza {}: {} ({} bytes, sha256 {})\n",
                index + 1,
                name,
                piece.file_size,
                piece.hash_value
            ));
            if let Some(note) = notes.get(&index) {
                message.push_str(&format!("      {note}\n"));
            }
        }
        if missing.len() > 20 {
            message.push_str(&format!("  ... y {} más\n", missing.len() - 20));
        }
        message.push_str("Comprueba --parts-dir o usa --download-missing.");
        return Err(Error::pieces(message));
    }

    let mut result = Vec::with_capacity(pieces.len());
    for (index, piece) in pieces.iter().enumerate() {
        let source = resolved[index].take().ok_or_else(|| {
            Error::new(EXIT_IO, "estado interno inconsistente al emparejar piezas")
        })?;
        if let PieceSource::Local(path) = &source {
            log.detail(format!(
                "pieza {}/{}: {} <- {}",
                index + 1,
                pieces.len(),
                util::url_basename(&piece.url).unwrap_or_default(),
                path.display()
            ));
        }
        result.push(LocatedPiece {
            index,
            piece: piece.clone(),
            source,
        });
    }

    let extra = candidates
        .iter()
        .filter(|path| !claimed.contains(&path_key(path)))
        .count();
    if extra > 0 {
        log.detail(format!(
            "{} archivo(s) .pkg no usados en las carpetas de piezas",
            extra
        ));
    }
    Ok(result)
}

fn collect_candidates(dirs: &[PathBuf], exclude: &[PathBuf], log: &Log) -> Result<Vec<PathBuf>> {
    let exclude_keys: HashSet<PathBuf> = exclude
        .iter()
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect();
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            return Err(Error::new(
                EXIT_IO,
                format!("no existe la carpeta de piezas '{}'", dir.display()),
            ));
        }
        let entries = fs::read_dir(dir)
            .map_err(|e| Error::io_path("no se pudo listar la carpeta de piezas", dir, e))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| Error::io_path("no se pudo leer una entrada de", dir, e))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "pkg" {
                continue;
            }
            let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if exclude_keys.contains(&key) {
                log.detail(format!(
                    "ignorando '{}' (archivo de salida)",
                    path.display()
                ));
                continue;
            }
            if seen.insert(key) {
                result.push(path);
            }
        }
    }
    Ok(result)
}

fn hash_file_with_bar(
    path: &Path,
    buffer: &mut [u8],
    bar: &ProgressBar,
) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    loop {
        let read = file.read(buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bar.inc(read as u64);
    }
    Ok(hex::encode(hasher.finalize()))
}
