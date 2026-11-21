mod cli;
mod error;
mod http;
mod journal;
mod locate;
mod log;
mod manifest;
mod merge;
mod progress;
mod util;
mod verify;

use crate::cli::Cli;
use crate::error::{Error, Result, EXIT_USAGE};
use crate::locate::{LocatedPiece, PieceSource};
use crate::log::Log;
use crate::merge::MergeRequest;
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() { EXIT_USAGE } else { 0 };
            let _ = error.print();
            std::process::exit(code);
        }
    };

    let code = match run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[error] {}", error.message);
            error.code
        }
    };
    std::process::exit(code);
}

fn run(cli: Cli) -> Result<()> {
    let log = Log::new(cli.log.as_deref(), cli.verbose)?;
    log.info(format!("pkg-merge {}", env!("CARGO_PKG_VERSION")));

    if cli.check && cli.in_place {
        return Err(Error::usage("--check no se puede combinar con --in-place"));
    }
    if cli.check && cli.delete_parts {
        return Err(Error::usage(
            "--check no se puede combinar con --delete-parts",
        ));
    }
    if cli.delete_parts && !cli.in_place {
        return Err(Error::usage(
            "--delete-parts solo se puede usar con --in-place",
        ));
    }
    if cli.download_missing && cli.check {
        log.warn("--check no descarga piezas; las que falten se reportarán como error.");
    }
    if cli.jobs == 0 || cli.jobs > 64 {
        return Err(Error::usage("--jobs debe estar entre 1 y 64"));
    }

    let input = resolve_input(&cli)?;
    let loaded = manifest::load(&input, &log)?;

    log.info(format!(
        "Origen del manifiesto: {}",
        loaded.source.describe()
    ));
    log.info(format!("Manifiesto: {}", loaded.base_name));
    log.info(format!(
        "Piezas: {} · Tamaño final: {} · packageDigest: {}",
        loaded.manifest.pieces.len(),
        util::format_bytes(loaded.manifest.original_file_size),
        loaded
            .manifest
            .package_digest
            .as_deref()
            .unwrap_or("(no indicado)")
    ));
    if let (Some(first), Some(last)) = (
        loaded.manifest.pieces.first(),
        loaded.manifest.pieces.last(),
    ) {
        log.detail(format!(
            "primera pieza: {}",
            util::url_basename(&first.url).unwrap_or_default()
        ));
        log.detail(format!(
            "última pieza: {}",
            util::url_basename(&last.url).unwrap_or_default()
        ));
    }

    let dirs = resolve_parts_dirs(&cli, &loaded);
    log.detail(format!(
        "carpetas de piezas: {}",
        dirs.iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let preliminary = preliminary_output(&cli, &loaded);
    let mut exclude: Vec<PathBuf> = Vec::new();
    if let manifest::Source::File(path) = &loaded.source {
        exclude.push(path.clone());
    }
    if let Some((directory, name)) = &preliminary {
        let final_path = directory.join(name);
        exclude.push(final_path.clone());
        let part = merge::part_path(&final_path);
        exclude.push(part.clone());
        exclude.push(merge::journal_path(&part));
        let stem = Path::new(name)
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "manifest".to_string());
        exclude.push(directory.join(format!("{stem}.manifest.json")));
    }

    let pieces = locate::locate_all(
        &loaded.manifest.pieces,
        &locate::LocateOptions {
            dirs: &dirs,
            exclude: &exclude,
            download_missing: cli.download_missing && !cli.check,
            no_progress: cli.no_progress,
        },
        &log,
    )?;

    if cli.check {
        return merge::check(&pieces, cli.jobs, cli.no_progress, &log);
    }

    let output = resolve_output(&cli, &loaded, &pieces)?;
    let part = merge::part_path(&output);
    for located in &pieces {
        if let PieceSource::Local(path) = &located.source {
            if util::same_path(path, &output) && !(cli.in_place && located.index == 0) {
                return Err(Error::usage(format!(
                    "la salida '{}' coincide con una pieza de entrada; usa --output en otra ruta",
                    output.display()
                )));
            }
            if util::same_path(path, &part) {
                return Err(Error::usage(format!(
                    "la salida temporal '{}' coincide con una pieza de entrada; usa --output en otra ruta",
                    part.display()
                )));
            }
        }
    }

    if !cli.no_save_manifest {
        save_manifest_copy(&output, &loaded, &log);
    }
    if cli.delete_parts {
        merge::confirm_delete(&pieces, cli.yes, &log)?;
    }

    merge::install_signal_handler()?;
    let summary = merge::run(
        &MergeRequest {
            manifest: &loaded.manifest,
            pieces: &pieces,
            final_path: output,
            in_place: cli.in_place,
            delete_parts: cli.delete_parts,
            verify: cli.verify,
            buffer_bytes: (cli.buffer as usize).saturating_mul(1024 * 1024),
            force: cli.force,
            strict_package_digest: cli.strict_package_digest,
            verify_final: cli.verify_final || cli.strict_package_digest,
            stop_after: cli.stop_after,
            no_progress: cli.no_progress,
        },
        &log,
    )?;

    log.info(format!("PKG generado: {}", summary.output.display()));
    log.info(format!(
        "Tamaño: {} · Tiempo: {} · Velocidad media: {}",
        util::format_bytes(summary.bytes),
        util::format_duration(summary.elapsed),
        util::format_speed(summary.bytes as f64 / summary.elapsed.as_secs_f64().max(0.001))
    ));
    if summary.deleted_parts > 0 {
        log.info(format!("Piezas borradas: {}", summary.deleted_parts));
    }
    if let Some(hash) = &summary.full_hash {
        log.info(format!("SHA-256 del archivo unido: {hash}"));
    }
    Ok(())
}

fn resolve_input(cli: &Cli) -> Result<String> {
    match (&cli.manifest, &cli.input) {
        (Some(_), Some(_)) => Err(Error::usage(
            "usa la entrada posicional o --manifest, pero no ambos",
        )),
        (Some(manifest), None) => Ok(manifest.clone()),
        (None, Some(input)) => Ok(input.clone()),
        (None, None) => {
            let cwd = std::env::current_dir()
                .map_err(|e| Error::io("no se pudo obtener el directorio actual", e))?;
            manifest::find_in_dir(&cwd).map(|path| path.display().to_string())
        }
    }
}

fn resolve_parts_dirs(cli: &Cli, loaded: &manifest::LoadedManifest) -> Vec<PathBuf> {
    if !cli.parts_dirs.is_empty() {
        return util::dedup_paths(&cli.parts_dirs);
    }
    let mut dirs = Vec::new();
    if let manifest::Source::File(path) = &loaded.source {
        if let Some(parent) = path.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    util::dedup_paths(&dirs)
}

fn preliminary_output(cli: &Cli, loaded: &manifest::LoadedManifest) -> Option<(PathBuf, String)> {
    let name = util::ensure_pkg_name(&loaded.base_name);
    if let Some(output) = &cli.output {
        if util::is_dir_like_output(output) {
            return Some((output.clone(), name));
        }
        let directory = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let file_name = output
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or(name);
        return Some((directory, file_name));
    }
    if cli.in_place {
        return None;
    }
    if let manifest::Source::File(path) = &loaded.source {
        if let Some(parent) = path.parent() {
            return Some((parent.to_path_buf(), name));
        }
    }
    std::env::current_dir().ok().map(|dir| (dir, name))
}

fn resolve_output(
    cli: &Cli,
    loaded: &manifest::LoadedManifest,
    pieces: &[LocatedPiece],
) -> Result<PathBuf> {
    let default_name = util::ensure_pkg_name(&loaded.base_name);
    let output = if let Some(output) = &cli.output {
        if util::is_dir_like_output(output) {
            fs::create_dir_all(output)
                .map_err(|e| Error::io_path("no se pudo crear la carpeta de salida", output, e))?;
            output.join(&default_name)
        } else {
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| {
                        Error::io_path("no se pudo crear la carpeta de salida", parent, e)
                    })?;
                }
            }
            output.clone()
        }
    } else if cli.in_place {
        match &pieces[0].source {
            PieceSource::Local(path) => {
                let directory = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                directory.join(&default_name)
            }
            PieceSource::Download => {
                return Err(Error::pieces(
                    "el modo --in-place necesita la pieza 0 en disco",
                ))
            }
        }
    } else if let manifest::Source::File(path) = &loaded.source {
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        directory.join(&default_name)
    } else {
        std::env::current_dir()
            .map_err(|e| Error::io("no se pudo obtener el directorio actual", e))?
            .join(&default_name)
    };

    if let Some(file_name) = output.file_name() {
        if !util::is_valid_piece_filename(&file_name.to_string_lossy()) {
            return Err(Error::usage(format!(
                "el nombre de salida '{}' no es válido",
                file_name.to_string_lossy()
            )));
        }
    }
    Ok(output)
}

fn save_manifest_copy(final_path: &Path, loaded: &manifest::LoadedManifest, log: &Log) {
    let directory = final_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = final_path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "manifest".to_string());
    let target = directory.join(format!("{stem}.manifest.json"));
    if let manifest::Source::File(source) = &loaded.source {
        if util::same_path(source, &target) {
            return;
        }
    }
    match fs::write(&target, loaded.raw.as_bytes()) {
        Ok(()) => log.info(format!(
            "Copia del manifiesto guardada en: {}",
            target.display()
        )),
        Err(error) => log.warn(format!(
            "no se pudo guardar la copia del manifiesto '{}': {error}",
            target.display()
        )),
    }
}
