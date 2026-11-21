use crate::cli::VerifyMode;
use crate::error::{Error, Result, EXIT_IO, EXIT_PIECES};
use crate::http;
use crate::journal::{self, Journal};
use crate::locate::{LocatedPiece, PieceSource};
use crate::manifest::Manifest;
use crate::progress;
use crate::util;
use crate::verify;
use crate::Log;
use indicatif::ProgressBar;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const SPACE_MARGIN: u64 = 64 * 1024 * 1024;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn install_signal_handler() -> Result<()> {
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .map_err(|e| {
        Error::new(
            EXIT_IO,
            format!("no se pudo instalar el manejador de Ctrl+C: {e}"),
        )
    })
}

fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

struct SleepGuard;

impl SleepGuard {
    fn new() -> Self {
        #[cfg(windows)]
        unsafe {
            let power = windows_sys::Win32::System::Power::ES_CONTINUOUS
                | windows_sys::Win32::System::Power::ES_SYSTEM_REQUIRED;
            windows_sys::Win32::System::Power::SetThreadExecutionState(power);
        }
        Self
    }
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Power::SetThreadExecutionState(
                windows_sys::Win32::System::Power::ES_CONTINUOUS,
            );
        }
    }
}

pub fn part_path(final_path: &Path) -> PathBuf {
    util::with_suffix(final_path, ".part")
}

pub fn journal_path(part: &Path) -> PathBuf {
    util::with_suffix(part, ".merge.json")
}

pub fn confirm_delete(pieces: &[LocatedPiece], yes: bool, log: &Log) -> Result<()> {
    let mut count = 0usize;
    let mut bytes = 0u64;
    for located in pieces.iter().skip(1) {
        if let PieceSource::Local(path) = &located.source {
            if path.exists() {
                count += 1;
                bytes += located.piece.file_size;
            }
        }
    }
    if count == 0 {
        return Ok(());
    }
    if yes {
        log.warn(format!(
            "Se borrarán {count} pieza(s) ({}) después de verificarlas.",
            util::format_bytes(bytes)
        ));
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Err(Error::usage(
            "--delete-parts necesita confirmación; ejecuta en una terminal o añade --yes",
        ));
    }
    eprint!(
        "Se borrarán {count} pieza(s) ({}) después de verificarlas. ¿Continuar? [s/N] ",
        util::format_bytes(bytes)
    );
    io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::io("no se pudo leer la confirmación", e))?;
    let answer = line.trim().to_ascii_lowercase();
    if matches!(answer.as_str(), "s" | "si" | "sí" | "y" | "yes") {
        Ok(())
    } else {
        Err(Error::usage("cancelado por el usuario"))
    }
}

pub struct MergeRequest<'a> {
    pub manifest: &'a Manifest,
    pub pieces: &'a [LocatedPiece],
    pub final_path: PathBuf,
    pub in_place: bool,
    pub delete_parts: bool,
    pub verify: VerifyMode,
    pub buffer_bytes: usize,
    pub force: bool,
    pub strict_package_digest: bool,
    pub verify_final: bool,
    pub stop_after: Option<usize>,
    pub no_progress: bool,
}

pub struct MergeSummary {
    pub output: PathBuf,
    pub bytes: u64,
    pub deleted_parts: usize,
    pub full_hash: Option<String>,
    pub elapsed: std::time::Duration,
}

pub fn run(request: &MergeRequest<'_>, log: &Log) -> Result<MergeSummary> {
    let started = Instant::now();
    let total_pieces = request.pieces.len();
    if total_pieces == 0 {
        return Err(Error::manifest("no hay piezas que unir"));
    }
    if request.delete_parts && !request.in_place {
        return Err(Error::usage(
            "--delete-parts solo se puede usar junto a --in-place",
        ));
    }

    let piece0 = &request.pieces[0];
    let piece0_local = match &piece0.source {
        PieceSource::Local(path) => Some(path.clone()),
        PieceSource::Download => None,
    };
    if request.in_place && piece0_local.is_none() {
        return Err(Error::pieces(
            "el modo --in-place necesita la pieza 0 en disco (no se puede descargar)",
        ));
    }

    let final_path = request.final_path.clone();
    let part = part_path(&final_path);
    let journal_path = journal_path(&part);
    if let Some(parent) = final_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::io_path("no se pudo crear la carpeta de salida", parent, e))?;
        }
    }
    let output_dir = final_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let buffer_bytes = request.buffer_bytes.max(1024 * 1024);
    let mut buffer = vec![0u8; buffer_bytes];

    let mut journal = journal::load(&journal_path)?;

    if request.in_place && request.force && piece0_local.is_none() && !part.exists() {
        return Err(Error::pieces(
            "no hay pieza 0 ni .part; no puedo reiniciar en --in-place",
        ));
    }

    let mut recovered_part = false;
    if request.force {
        journal::remove(&journal_path)?;
        journal = None;

        let final_exists = final_path.exists();
        let final_is_piece0 = request.in_place
            && piece0_local
                .as_ref()
                .map(|path| final_exists && util::same_path(path, &final_path))
                .unwrap_or(false);
        if final_exists && !final_is_piece0 {
            fs::remove_file(&final_path).map_err(|e| {
                Error::io_path("no se pudo borrar la salida anterior", &final_path, e)
            })?;
        }

        if part.exists() {
            if request.in_place {
                let piece0_exists = piece0_local
                    .as_ref()
                    .map(|path| path.exists())
                    .unwrap_or(false);
                if piece0_exists {
                    fs::remove_file(&part).map_err(|e| {
                        Error::io_path("no se pudo borrar el .part anterior", &part, e)
                    })?;
                } else {
                    let file = OpenOptions::new().write(true).open(&part).map_err(|e| {
                        Error::io_path("no se pudo abrir el .part para recuperarlo", &part, e)
                    })?;
                    file.set_len(piece0.piece.file_size).map_err(|e| {
                        Error::io_path("no se pudo truncar el .part a la pieza 0", &part, e)
                    })?;
                    file.sync_all().ok();
                    recovered_part = true;
                }
            } else {
                fs::remove_file(&part)
                    .map_err(|e| Error::io_path("no se pudo borrar el .part anterior", &part, e))?;
            }
        }
    } else if journal.is_none() {
        if part.exists() {
            return Err(Error::usage(format!(
                "ya existe '{}' sin journal. Usa --force para reiniciar.",
                part.display()
            )));
        }
        let final_exists = final_path.exists();
        let final_is_piece0 = request.in_place
            && piece0_local
                .as_ref()
                .map(|path| final_exists && util::same_path(path, &final_path))
                .unwrap_or(false);
        if final_exists && !final_is_piece0 {
            return Err(Error::usage(format!(
                "ya existe '{}'. Usa --force para sobrescribir o --output para elegir otro destino.",
                final_path.display()
            )));
        }
    }

    let has_journal = journal.is_some();
    let mut start_index = 0usize;
    let mut bytes_written = 0u64;

    if let Some(current) = journal.as_ref() {
        validate_journal(current, request)?;
        start_index = current.completed_pieces;
        bytes_written = current.bytes_written;
    }

    if has_journal && !part.exists() {
        return Err(Error::new(
            EXIT_IO,
            format!("no existe el archivo temporal '{}'", part.display()),
        ));
    }
    if !has_journal && !request.in_place && part.exists() && !recovered_part {
        return Err(Error::usage(format!(
            "ya existe '{}' sin journal. Usa --force para reiniciar.",
            part.display()
        )));
    }
    if has_journal && final_path.exists() {
        return Err(Error::usage(format!(
            "ya existe '{}' y además hay un merge a medias. Usa --force para reiniciar.",
            final_path.display()
        )));
    }

    // Preparación fresca del modo in-place.
    if journal.is_none() && request.in_place {
        let mut ready = false;
        if let Some(path) = &piece0_local {
            if path.exists() {
                if request.verify != VerifyMode::None {
                    let bar = progress::make_bar(
                        piece0.piece.file_size,
                        !request.no_progress,
                        0,
                        "verificando pieza 1",
                    );
                    let hash = match hash_file_into(path, &mut buffer, Some(&bar)) {
                        Ok(hash) => hash,
                        Err(error) => {
                            bar.abandon();
                            return Err(Error::io_path(
                                "no se pudo verificar la pieza 0",
                                path,
                                error,
                            ));
                        }
                    };
                    bar.finish_and_clear();
                    if !verify::hex_eq(&hash, &piece0.piece.hash_value) {
                        return Err(Error::pieces(format!(
                            "la pieza 1 ('{}') no coincide con su SHA-256.\n  esperado: {}\n  obtenido: {}\nNo se ha tocado el archivo original.",
                            path.display(),
                            piece0.piece.hash_value,
                            hash
                        )));
                    }
                }
                if part.exists() {
                    fs::remove_file(&part).map_err(|e| {
                        Error::io_path("no se pudo borrar el .part anterior", &part, e)
                    })?;
                }
                fs::rename(path, &part).map_err(|e| {
                    Error::new(
                        EXIT_IO,
                        format!(
                            "no se pudo mover la pieza 0 a '{}': {e}. ¿Está en el mismo disco? Usa --output en el mismo volumen.",
                            part.display()
                        ),
                    )
                })?;
                start_index = 1;
                bytes_written = piece0.piece.file_size;
                journal = Some(Journal::new(
                    "in-place",
                    request.manifest.original_file_size,
                    request.manifest.package_digest.clone(),
                    1,
                    bytes_written,
                ));
                journal::save(&journal_path, journal.as_ref().unwrap())?;
                log.info(format!(
                    "[ok] pieza 1/{} {} (reutilizada como base)",
                    total_pieces,
                    util::url_basename(&piece0.piece.url).unwrap_or_default()
                ));
                ready = true;
            }
        }
        if !ready {
            if !part.exists() {
                return Err(Error::pieces(
                    "falta la pieza 0 y no existe un .part para --in-place",
                ));
            }
            let meta = fs::metadata(&part)
                .map_err(|e| Error::io_path("no se pudo leer el .part", &part, e))?;
            if meta.len() < piece0.piece.file_size {
                return Err(Error::pieces(format!(
                    "el .part mide {} bytes y la pieza 0 necesita {}",
                    meta.len(),
                    piece0.piece.file_size
                )));
            }
            if request.verify != VerifyMode::None {
                let hash = verify::hash_range(&part, 0, piece0.piece.file_size, &mut buffer)
                    .map_err(|e| {
                        Error::io_path("no se pudo verificar la pieza 0 del .part", &part, e)
                    })?;
                if !verify::hex_eq(&hash, &piece0.piece.hash_value) {
                    return Err(Error::pieces(format!(
                        "la pieza 1 del .part no coincide con su SHA-256.\n  esperado: {}\n  obtenido: {}\nUsa --force para reiniciar desde cero.",
                        piece0.piece.hash_value, hash
                    )));
                }
            }
            start_index = 1;
            bytes_written = piece0.piece.file_size;
            journal = Some(Journal::new(
                "in-place",
                request.manifest.original_file_size,
                request.manifest.package_digest.clone(),
                1,
                bytes_written,
            ));
            journal::save(&journal_path, journal.as_ref().unwrap())?;
            log.info(format!(
                "[ok] pieza 1/{} {} (recuperada del .part)",
                total_pieces,
                util::url_basename(&piece0.piece.url).unwrap_or_default()
            ));
        }
    }

    if journal.is_none() {
        journal = Some(Journal::new(
            if request.in_place {
                "in-place"
            } else {
                "normal"
            },
            request.manifest.original_file_size,
            request.manifest.package_digest.clone(),
            start_index,
            bytes_written,
        ));
        journal::save(&journal_path, journal.as_ref().unwrap())?;
    }

    let resumed_run = has_journal && start_index > 0;

    // Al reanudar, verificamos la última pieza completada antes de continuar.
    if resumed_run {
        let last = &request.pieces[start_index - 1];
        let bar = progress::make_bar(
            last.piece.file_size,
            !request.no_progress,
            0,
            format!("verificando pieza {} (reanudación)", start_index),
        );
        let hash = hash_range_into(
            &part,
            last.piece.file_offset,
            last.piece.file_size,
            &mut buffer,
            Some(&bar),
        );
        let hash = match hash {
            Ok(hash) => hash,
            Err(error) => {
                bar.abandon();
                return Err(Error::io_path(
                    "no se pudo verificar el estado anterior",
                    &part,
                    error,
                ));
            }
        };
        bar.finish_and_clear();
        if request.verify != VerifyMode::None && !verify::hex_eq(&hash, &last.piece.hash_value) {
            return Err(Error::new(
                EXIT_PIECES,
                format!(
                    "el .part no coincide con la pieza {} ya completada; usa --force para reiniciar.",
                    start_index
                ),
            ));
        }
        if request.delete_parts {
            for completed in request.pieces.iter().take(start_index).skip(1) {
                if let PieceSource::Local(path) = &completed.source {
                    if path.exists() {
                        if let Err(error) = fs::remove_file(path) {
                            log.warn(format!("no se pudo borrar '{}': {error}", path.display()));
                        }
                    }
                }
            }
        }
    }

    let existing_len = if part.exists() {
        fs::metadata(&part)
            .map_err(|e| Error::io_path("no se pudo leer el .part", &part, e))?
            .len()
    } else {
        0
    };
    if resumed_run && existing_len < bytes_written {
        return Err(Error::new(
            EXIT_IO,
            format!(
                "el .part mide {existing_len} bytes y el journal dice {bytes_written}; usa --force"
            ),
        ));
    }
    if request.in_place {
        if request.delete_parts {
            let max_piece = request
                .pieces
                .iter()
                .skip(1)
                .map(|located| located.piece.file_size)
                .max()
                .unwrap_or(0);
            check_space(&output_dir, max_piece + SPACE_MARGIN, log)?;
        } else {
            let needed = request
                .manifest
                .original_file_size
                .saturating_sub(existing_len);
            check_space(&output_dir, needed + SPACE_MARGIN, log)?;
        }
    } else {
        let needed = request
            .manifest
            .original_file_size
            .saturating_sub(existing_len);
        check_space(&output_dir, needed + SPACE_MARGIN, log)?;
    }

    let reopen_existing = has_journal || request.in_place || recovered_part;
    let mut part_file = if reopen_existing {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&part)
            .map_err(|e| Error::io_path("no se pudo abrir el .part", &part, e))?
    } else {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&part)
            .map_err(|e| Error::io_path("no se pudo crear el .part", &part, e))?
    };

    if !request.in_place && existing_len != request.manifest.original_file_size {
        part_file
            .set_len(request.manifest.original_file_size)
            .map_err(|e| {
                Error::new(
                    EXIT_IO,
                    format!("no se pudo reservar espacio para '{}': {e}", part.display()),
                )
            })?;
    }

    let bar = progress::make_bar(
        request.manifest.original_file_size,
        !request.no_progress,
        bytes_written,
        "fusionando",
    );
    let _sleep_guard = SleepGuard::new();
    let mut completed = start_index;
    let mut deleted_parts = 0usize;

    for (index, located) in request.pieces.iter().enumerate().skip(start_index) {
        if interrupted() {
            let _ = part_file.sync_all();
            if let Some(current) = journal.as_mut() {
                current.touch(completed, bytes_written);
            }
            save_journal(&journal_path, &journal)?;
            bar.abandon();
            return Err(Error::interrupted(
                "interrumpido. Vuelve a ejecutar el mismo comando para reanudar.",
            ));
        }

        bar.set_message(format!("pieza {}/{}", index + 1, total_pieces));

        if request.in_place && request.delete_parts {
            if let Err(error) =
                check_space(&output_dir, located.piece.file_size + SPACE_MARGIN, log)
            {
                bar.abandon();
                return Err(error);
            }
        }

        if let Err(error) = part_file.seek(SeekFrom::Start(located.piece.file_offset)) {
            bar.abandon();
            return Err(Error::io_path(
                "no se pudo posicionar el archivo final",
                &part,
                error,
            ));
        }

        let mut piece_hasher = Sha256::new();
        let verify_piece_after = match &located.source {
            PieceSource::Local(_) => request.verify == VerifyMode::Post,
            PieceSource::Download => request.verify != VerifyMode::None,
        };

        let copy_result: io::Result<()> = match &located.source {
            PieceSource::Local(path) => {
                let meta = match fs::metadata(path) {
                    Ok(meta) => meta,
                    Err(error) => {
                        bar.abandon();
                        return Err(Error::io_path("no se pudo leer la pieza", path, error));
                    }
                };
                if meta.len() != located.piece.file_size {
                    bar.abandon();
                    return Err(Error::pieces(format!(
                        "la pieza {} ('{}') mide {} bytes y el manifiesto dice {}",
                        index + 1,
                        path.display(),
                        meta.len(),
                        located.piece.file_size
                    )));
                }
                if request.verify == VerifyMode::Pre {
                    let pre = progress::make_bar(
                        located.piece.file_size,
                        !request.no_progress,
                        0,
                        format!("verificando pieza {}", index + 1),
                    );
                    let hash = match hash_file_into(path, &mut buffer, Some(&pre)) {
                        Ok(hash) => hash,
                        Err(error) => {
                            pre.abandon();
                            bar.abandon();
                            return Err(Error::io_path(
                                "no se pudo verificar la pieza",
                                path,
                                error,
                            ));
                        }
                    };
                    pre.finish_and_clear();
                    if !verify::hex_eq(&hash, &located.piece.hash_value) {
                        bar.abandon();
                        return Err(Error::pieces(format!(
                            "la pieza {}/{} ('{}') no coincide con su SHA-256.\n  esperado: {}\n  obtenido: {}",
                            index + 1,
                            total_pieces,
                            path.display(),
                            located.piece.hash_value,
                            hash
                        )));
                    }
                }
                let mut source = match File::open(path) {
                    Ok(file) => file,
                    Err(error) => {
                        bar.abandon();
                        return Err(Error::io_path("no se pudo abrir la pieza", path, error));
                    }
                };
                copy_chunks(
                    &mut source,
                    &mut part_file,
                    &mut buffer,
                    located.piece.file_size,
                    if verify_piece_after {
                        Some(&mut piece_hasher)
                    } else {
                        None
                    },
                    &bar,
                )
            }
            PieceSource::Download => {
                let mut download = match http::get(&located.piece.url, EXIT_PIECES) {
                    Ok(download) => download,
                    Err(error) => {
                        bar.abandon();
                        return Err(error);
                    }
                };
                if let Some(len) = download.len {
                    if len != located.piece.file_size {
                        bar.abandon();
                        return Err(Error::pieces(format!(
                            "la descarga de la pieza {} anuncia {} bytes y el manifiesto dice {}",
                            index + 1,
                            len,
                            located.piece.file_size
                        )));
                    }
                }
                copy_chunks(
                    download.reader.as_mut(),
                    &mut part_file,
                    &mut buffer,
                    located.piece.file_size,
                    if verify_piece_after {
                        Some(&mut piece_hasher)
                    } else {
                        None
                    },
                    &bar,
                )
            }
        };

        match copy_result {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                let _ = part_file.sync_all();
                if let Some(current) = journal.as_mut() {
                    current.touch(completed, bytes_written);
                }
                save_journal(&journal_path, &journal)?;
                bar.abandon();
                return Err(Error::interrupted(
                    "interrumpido. Vuelve a ejecutar el mismo comando para reanudar.",
                ));
            }
            Err(error) => {
                bar.abandon();
                let source = match &located.source {
                    PieceSource::Local(path) => path.display().to_string(),
                    PieceSource::Download => located.piece.url.clone(),
                };
                return Err(Error::new(
                    EXIT_IO,
                    format!(
                        "error copiando la pieza {} desde {}: {error}",
                        index + 1,
                        source
                    ),
                ));
            }
        }

        if verify_piece_after {
            let actual = hex::encode(piece_hasher.finalize());
            if !verify::hex_eq(&actual, &located.piece.hash_value) {
                let name = util::url_basename(&located.piece.url).unwrap_or_default();
                let _ = part_file.sync_all();
                if let Some(current) = journal.as_mut() {
                    current.touch(completed, bytes_written);
                }
                save_journal(&journal_path, &journal)?;
                bar.abandon();
                return Err(Error::pieces(format!(
                    "la pieza {}/{} ({}) no coincide con su SHA-256.\n  esperado: {}\n  obtenido: {}\nEl archivo final no se ha generado. Corrige o vuelve a descargar esa pieza.",
                    index + 1,
                    total_pieces,
                    name,
                    located.piece.hash_value,
                    actual
                )));
            }
        }

        completed = index + 1;
        bytes_written = located.piece.file_offset + located.piece.file_size;
        if let Some(current) = journal.as_mut() {
            current.touch(completed, bytes_written);
        }
        save_journal(&journal_path, &journal)?;
        bar.println(format!(
            "[ok] pieza {}/{} {}",
            index + 1,
            total_pieces,
            util::url_basename(&located.piece.url).unwrap_or_default()
        ));

        if request.delete_parts && index > 0 {
            if let PieceSource::Local(path) = &located.source {
                if let Err(error) = part_file.sync_all() {
                    log.warn(format!(
                        "no se pudo sincronizar antes de borrar '{}': {error}",
                        path.display()
                    ));
                }
                match fs::remove_file(path) {
                    Ok(()) => {
                        deleted_parts += 1;
                        log.detail(format!("borrada '{}'", path.display()));
                    }
                    Err(error) => {
                        log.warn(format!("no se pudo borrar '{}': {error}", path.display()))
                    }
                }
            }
        }

        if let Some(stop) = request.stop_after {
            if completed >= stop {
                let _ = part_file.sync_all();
                if let Some(current) = journal.as_mut() {
                    current.touch(completed, bytes_written);
                }
                save_journal(&journal_path, &journal)?;
                bar.abandon();
                return Err(Error::interrupted(format!(
                    "detenido tras {completed} pieza(s) para pruebas. Vuelve a ejecutar para reanudar."
                )));
            }
        }
    }

    if let Err(error) = part_file.sync_all() {
        bar.abandon();
        return Err(Error::io_path(
            "no se pudo sincronizar el archivo final",
            &part,
            error,
        ));
    }
    drop(part_file);

    verify_final_size(&part, request.manifest.original_file_size, log)?;

    let expected_digest = request.manifest.package_digest.as_deref();
    let full_hash = if request.verify_final || request.strict_package_digest {
        Some(verify_final_file(
            &part,
            request.pieces,
            &mut buffer,
            request.no_progress,
            log,
        )?)
    } else {
        None
    };

    if let Some(actual) = &full_hash {
        if let Some(expected) = expected_digest {
            if verify::hex_eq(actual, expected) {
                log.info("El packageDigest de Sony coincide con el SHA-256 del archivo unido.");
            } else if request.strict_package_digest {
                bar.abandon();
                return Err(Error::digest(format!(
                    "el packageDigest de Sony no coincide con el SHA-256 del archivo unido.\n  esperado: {expected}\n  obtenido: {actual}\nSe pidió --strict-package-digest; el .part se conserva en '{}'.",
                    part.display()
                )));
            } else {
                log.warn(format!(
                    "El packageDigest de Sony no coincide con el SHA-256 del archivo unido.\n  esperado: {expected}\n  obtenido: {actual}\nEsto es normal en PKG divididos de PS5 (FIH + _sc): las piezas y el tamaño total sí están verificados. Usa --strict-package-digest para convertirlo en error."
                ));
            }
        } else if request.strict_package_digest {
            log.warn("El manifiesto no trae packageDigest; no hay nada que comparar.");
        } else {
            log.info(
                "El manifiesto no trae packageDigest; se verificó el archivo final pieza a pieza.",
            );
        }
    } else if expected_digest.is_some() {
        log.detail(
            "packageDigest no comprobado; usa --verify-final para releer y verificar el archivo, o --strict-package-digest para compararlo.",
        );
    }

    if final_path.exists() {
        fs::remove_file(&final_path).map_err(|e| {
            Error::io_path("no se pudo reemplazar la salida existente", &final_path, e)
        })?;
    }
    fs::rename(&part, &final_path).map_err(|e| {
        Error::new(
            EXIT_IO,
            format!(
                "no se pudo renombrar '{}' a '{}': {e}",
                part.display(),
                final_path.display()
            ),
        )
    })?;
    if let Err(error) = journal::remove(&journal_path) {
        log.warn(format!(
            "el PKG está listo, pero no se pudo borrar el journal: {}",
            error.message
        ));
    }
    bar.finish_and_clear();

    Ok(MergeSummary {
        output: final_path,
        bytes: request.manifest.original_file_size,
        deleted_parts,
        full_hash,
        elapsed: started.elapsed(),
    })
}

fn verify_final_size(part: &Path, expected: u64, log: &Log) -> Result<()> {
    let actual = fs::metadata(part)
        .map_err(|e| Error::io_path("no se pudo leer el tamaño del archivo final", part, e))?
        .len();
    if actual != expected {
        return Err(Error::pieces(format!(
            "el tamaño del archivo final no coincide.\n  esperado: {expected} bytes\n  obtenido: {actual} bytes\nEl .part se conserva en '{}'.",
            part.display()
        )));
    }
    log.info(format!(
        "Tamaño total verificado: {} ({} bytes).",
        util::format_bytes(actual),
        actual
    ));
    Ok(())
}

fn verify_final_file(
    part: &Path,
    pieces: &[LocatedPiece],
    buffer: &mut [u8],
    no_progress: bool,
    log: &Log,
) -> Result<String> {
    let total: u64 = pieces.iter().map(|located| located.piece.file_size).sum();
    let bar = progress::make_bar(total, !no_progress, 0, "verificando archivo final");
    let mut file = File::open(part)
        .map_err(|e| Error::io_path("no se pudo abrir el archivo final", part, e))?;
    let mut global = Sha256::new();

    for (index, located) in pieces.iter().enumerate() {
        file.seek(SeekFrom::Start(located.piece.file_offset))
            .map_err(|e| Error::io_path("no se pudo posicionar el archivo final", part, e))?;
        let mut hasher = Sha256::new();
        let mut remaining = located.piece.file_size;
        while remaining > 0 {
            let want = if remaining < buffer.len() as u64 {
                remaining as usize
            } else {
                buffer.len()
            };
            let read = file
                .read(&mut buffer[..want])
                .map_err(|e| Error::io_path("error leyendo el archivo final", part, e))?;
            if read == 0 {
                bar.abandon();
                return Err(Error::pieces(format!(
                    "el archivo final termina antes de la pieza {}/{}",
                    index + 1,
                    pieces.len()
                )));
            }
            hasher.update(&buffer[..read]);
            global.update(&buffer[..read]);
            bar.inc(read as u64);
            remaining -= read as u64;
        }
        let actual = hex::encode(hasher.finalize());
        if !verify::hex_eq(&actual, &located.piece.hash_value) {
            bar.abandon();
            return Err(Error::pieces(format!(
                "la verificación final de la pieza {}/{} falló.\n  esperado: {}\n  obtenido: {}",
                index + 1,
                pieces.len(),
                located.piece.hash_value,
                actual
            )));
        }
    }

    bar.finish_and_clear();
    let actual = hex::encode(global.finalize());
    log.info(format!(
        "Verificación final correcta: {} pieza(s), {} releídos.",
        pieces.len(),
        util::format_bytes(total)
    ));
    Ok(actual)
}

fn validate_journal(journal: &Journal, request: &MergeRequest<'_>) -> Result<()> {
    if journal.version != 1 {
        return Err(Error::new(
            EXIT_IO,
            "journal con versión desconocida; usa --force",
        ));
    }
    let expected_mode = if request.in_place {
        "in-place"
    } else {
        "normal"
    };
    if journal.mode != expected_mode {
        return Err(Error::new(
            EXIT_IO,
            format!(
                "el journal es del modo '{}' y ahora usas '{}'; usa --force",
                journal.mode, expected_mode
            ),
        ));
    }
    if journal.original_file_size != request.manifest.original_file_size
        || !digest_matches(
            journal.package_digest.as_deref(),
            request.manifest.package_digest.as_deref(),
        )
    {
        return Err(Error::new(
            EXIT_IO,
            "el journal pertenece a otro manifiesto; usa --force",
        ));
    }
    if journal.completed_pieces > request.pieces.len() {
        return Err(Error::new(EXIT_IO, "journal inconsistente; usa --force"));
    }
    if journal.completed_pieces == 0 {
        if journal.bytes_written != 0 {
            return Err(Error::new(EXIT_IO, "journal inconsistente; usa --force"));
        }
    } else {
        let last = &request.pieces[journal.completed_pieces - 1];
        let expected = last.piece.file_offset + last.piece.file_size;
        if journal.bytes_written != expected {
            return Err(Error::new(EXIT_IO, "journal inconsistente; usa --force"));
        }
    }
    Ok(())
}

fn digest_matches(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => verify::hex_eq(x, y),
        _ => false,
    }
}

fn save_journal(path: &Path, journal: &Option<Journal>) -> Result<()> {
    if let Some(current) = journal {
        journal::save(path, current)?;
    }
    Ok(())
}

fn check_space(directory: &Path, needed: u64, log: &Log) -> Result<()> {
    let available = match util::available_space(directory) {
        Ok(value) => value,
        Err(error) => {
            log.warn(format!(
                "no se pudo comprobar el espacio libre: {}",
                error.message
            ));
            return Ok(());
        }
    };
    if available < needed {
        return Err(Error::new(
            EXIT_IO,
            format!(
                "espacio insuficiente en '{}': necesito {} y hay {}. Libera espacio o usa --output en otro disco.",
                directory.display(),
                util::format_bytes(needed),
                util::format_bytes(available)
            ),
        ));
    }
    log.detail(format!(
        "espacio libre en '{}': {} (necesario: {})",
        directory.display(),
        util::format_bytes(available),
        util::format_bytes(needed)
    ));
    Ok(())
}

fn hash_file_into(path: &Path, buffer: &mut [u8], bar: Option<&ProgressBar>) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    loop {
        let read = file.read(buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        if let Some(bar) = bar {
            bar.inc(read as u64);
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

fn hash_range_into(
    path: &Path,
    offset: u64,
    length: u64,
    buffer: &mut [u8],
    bar: Option<&ProgressBar>,
) -> io::Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut hasher = Sha256::new();
    let mut remaining = length;
    while remaining > 0 {
        let want = if remaining < buffer.len() as u64 {
            remaining as usize
        } else {
            buffer.len()
        };
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "fin de archivo inesperado",
            ));
        }
        hasher.update(&buffer[..read]);
        if let Some(bar) = bar {
            bar.inc(read as u64);
        }
        remaining -= read as u64;
    }
    Ok(hex::encode(hasher.finalize()))
}

fn copy_chunks<R: Read + ?Sized>(
    source: &mut R,
    destination: &mut File,
    buffer: &mut [u8],
    mut remaining: u64,
    mut piece_hasher: Option<&mut Sha256>,
    bar: &ProgressBar,
) -> io::Result<()> {
    while remaining > 0 {
        if interrupted() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "interrumpido"));
        }
        let want = if remaining < buffer.len() as u64 {
            remaining as usize
        } else {
            buffer.len()
        };
        let read = source.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "la pieza termina antes de lo esperado",
            ));
        }
        if let Some(hasher) = piece_hasher.as_deref_mut() {
            hasher.update(&buffer[..read]);
        }
        destination.write_all(&buffer[..read])?;
        remaining -= read as u64;
        bar.inc(read as u64);
    }
    Ok(())
}

pub fn check(pieces: &[LocatedPiece], jobs: usize, no_progress: bool, log: &Log) -> Result<()> {
    let mut total = 0u64;
    let mut pending_downloads = 0usize;
    for located in pieces {
        match located.source {
            PieceSource::Local(_) => total += located.piece.file_size,
            PieceSource::Download => pending_downloads += 1,
        }
    }
    if pending_downloads > 0 {
        return Err(Error::pieces(format!(
            "faltan {pending_downloads} pieza(s); --check no descarga. Colócalas en disco o usa un merge con --download-missing."
        )));
    }

    let bar = progress::make_bar(total, !no_progress, 0, "verificando piezas");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()
        .map_err(|e| Error::new(EXIT_IO, format!("no se pudo crear el pool de hilos: {e}")))?;

    let results: Vec<(usize, String, bool, String)> = pool.install(|| {
        use rayon::prelude::*;
        pieces
            .par_iter()
            .map(|located| {
                let bar = bar.clone();
                let mut buffer = vec![0u8; 1024 * 1024];
                match &located.source {
                    PieceSource::Local(path) => {
                        let name = path.display().to_string();
                        match hash_file_into(path, &mut buffer, Some(&bar)) {
                            Ok(actual) => {
                                let ok = verify::hex_eq(&actual, &located.piece.hash_value);
                                (located.index, name, ok, actual)
                            }
                            Err(error) => (
                                located.index,
                                name,
                                false,
                                format!("error de lectura: {error}"),
                            ),
                        }
                    }
                    PieceSource::Download => (
                        located.index,
                        located.piece.url.clone(),
                        false,
                        "pendiente de descarga".to_string(),
                    ),
                }
            })
            .collect()
    });
    bar.finish_and_clear();

    let mut failed = 0usize;
    for (index, name, ok, detail) in results {
        if ok {
            log.info(format!(
                "[ok] pieza {}/{} {}",
                index + 1,
                pieces.len(),
                name
            ));
        } else {
            failed += 1;
            let expected = &pieces[index].piece.hash_value;
            log.error(format!(
                "[fallo] pieza {}/{} {}\n  esperado: {expected}\n  obtenido: {detail}",
                index + 1,
                pieces.len(),
                name
            ));
        }
    }
    if failed > 0 {
        return Err(Error::pieces(format!(
            "{failed} pieza(s) no coinciden con su SHA-256"
        )));
    }
    log.info(format!(
        "Todo correcto: {} pieza(s), {} verificados con SHA-256.",
        pieces.len(),
        util::format_bytes(total)
    ));
    Ok(())
}
