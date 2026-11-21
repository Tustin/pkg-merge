use crate::error::{Error, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

pub fn format_speed(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second <= 0.0 {
        return "0 B/s".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_second as u64))
}

pub fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn looks_like_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Cambia la extensión .crc por .json conservando query/fragmento.
pub fn crc_to_json_url(url: &str) -> String {
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    let (base, suffix) = url.split_at(cut);
    if base.to_ascii_lowercase().ends_with(".crc") {
        format!("{}.json{}", &base[..base.len() - 4], suffix)
    } else {
        url.to_string()
    }
}

/// Último segmento de la URL, decodificado y listo para usar como nombre local.
pub fn url_basename(url: &str) -> Option<String> {
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    let path = &url[..cut];
    let segment = path.rsplit('/').next()?;
    if segment.is_empty() {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .ok()?
        .into_owned();
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

pub fn strip_json_ext(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".json") {
        name[..name.len() - 5].to_string()
    } else {
        name.to_string()
    }
}

pub fn ensure_pkg_name(name: &str) -> String {
    let mut name = strip_json_ext(name);
    if !name.to_ascii_lowercase().ends_with(".pkg") {
        name.push_str(".pkg");
    }
    name
}

fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

/// Comprueba que un nombre se pueda usar como archivo en Windows sin sorpresas.
pub fn is_valid_piece_filename(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return false;
    }
    !is_reserved_windows_name(name)
}

pub fn is_valid_output_stem(stem: &str) -> bool {
    is_valid_piece_filename(stem)
}

pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

pub fn available_space(path: &Path) -> Result<u64> {
    let target = existing_ancestor(path).unwrap_or_else(|| PathBuf::from("."));
    fs2::available_space(&target)
        .map_err(|e| Error::io_path("no se pudo consultar el espacio libre en", &target, e))
}

pub fn dedup_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        let key = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        if seen.insert(key) {
            result.push(path.clone());
        }
    }
    result
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Decide si --output se refiere a una carpeta o a un archivo.
pub fn is_dir_like_output(path: &Path) -> bool {
    if path.exists() {
        return path.is_dir();
    }
    let text = path.to_string_lossy();
    if text.ends_with('/') || text.ends_with('\\') {
        return true;
    }
    path.extension().is_none()
}
