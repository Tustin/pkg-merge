use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VerifyMode {
    /// Copia y calcula el SHA-256 en la misma pasada (recomendado).
    Post,
    /// Hashea cada pieza antes de copiarla (dos lecturas).
    Pre,
    /// No verifica hashes (más rápido, no recomendado).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FinalDigest {
    /// Usa el hash calculado durante la copia; si se reanudó, se omite con aviso.
    Auto,
    /// Relee el archivo final completo para verificar el SHA-256 global.
    Rehash,
    /// No verifica el SHA-256 global.
    Skip,
}

const AFTER_HELP: &str = r#"EJEMPLOS:
  pkg-merge https://.../EP0082-PPSA08668_00-EUROPE0000000000.crc --parts-dir Z:\pieces --output C:\out
  pkg-merge Z:\pieces --output C:\out
  pkg-merge Z:\pieces --in-place --delete-parts --yes
  pkg-merge https://.../EP0082-PPSA08668_00-EUROPE0000000000.json --check

MODO FÁCIL:
  1. Abre la URL del .crc en el navegador y cambia .crc por .json.
  2. Guarda ese .json en la carpeta de las piezas.
  3. Arrastra la carpeta sobre pkg-merge.exe.

SEGURIDAD:
  El archivo final solo se renombra a .pkg si todas las piezas y el SHA-256
  global son correctos. --delete-parts borra las piezas después de verificarlas
  y solo funciona junto a --in-place."#;

#[derive(Debug, Parser)]
#[command(
    name = "pkg-merge",
    version,
    about = "Une las piezas de un PKG de PS4/PS5 usando el manifiesto oficial",
    long_about = "pkg-merge lee el manifiesto .json de Sony y coloca cada pieza en su\n\
                  fileOffset exacto. Verifica el SHA-256 de cada pieza y el packageDigest\n\
                  global, y solo deja un .pkg con nombre final si todo es correcto.\n\
                  Funciona con juegos base, parches y DLC de PS4 y PS5.",
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// URL .crc/.json, archivo .json o carpeta con las piezas.
    #[arg(value_name = "ENTRADA")]
    pub input: Option<String>,

    /// Carpeta o archivo .pkg de salida (por defecto, junto al manifiesto o cwd).
    #[arg(long, value_name = "RUTA")]
    pub output: Option<PathBuf>,

    /// Carpeta donde están las piezas (se puede repetir).
    #[arg(long = "parts-dir", value_name = "DIR", action = clap::ArgAction::Append)]
    pub parts_dirs: Vec<PathBuf>,

    /// Manifiesto explícito (.json o URL), en lugar de la entrada posicional.
    #[arg(long, value_name = "URL|RUTA")]
    pub manifest: Option<String>,

    /// Usa la pieza 0 como archivo final, sin duplicar los 100+ GB.
    #[arg(long)]
    pub in_place: bool,

    /// Borra cada pieza después de escribirla y verificarla. Requiere --in-place.
    #[arg(long)]
    pub delete_parts: bool,

    /// Modo de verificación SHA-256.
    #[arg(long, value_enum, default_value_t = VerifyMode::Post)]
    pub verify: VerifyMode,

    /// Solo verifica el manifiesto y las piezas; no escribe nada.
    #[arg(long)]
    pub check: bool,

    /// Búfer de lectura/escritura en MiB.
    #[arg(long, default_value_t = 64, value_parser = clap::value_parser!(u64).range(1..=1024), value_name = "MB")]
    pub buffer: u64,

    /// Hilos para --check (1 desactiva el paralelismo).
    #[arg(long, default_value_t = 4, value_name = "N")]
    pub jobs: usize,

    /// Sobrescribe la salida y reinicia un merge anterior.
    #[arg(long)]
    pub force: bool,

    /// No pide confirmación para --delete-parts.
    #[arg(long)]
    pub yes: bool,

    /// Oculta la barra de progreso (útil para scripts).
    #[arg(long)]
    pub no_progress: bool,

    /// Guarda un log detallado en un archivo.
    #[arg(long, value_name = "ARCHIVO")]
    pub log: Option<PathBuf>,

    /// Muestra detalles adicionales.
    #[arg(short, long)]
    pub verbose: bool,

    /// Si el packageDigest no coincide, avisa pero deja el archivo final.
    #[arg(long)]
    pub allow_digest_mismatch: bool,

    /// Cómo verificar el SHA-256 global.
    #[arg(long, value_enum, default_value_t = FinalDigest::Auto)]
    pub final_digest: FinalDigest,

    /// Descarga las piezas que falten directamente al archivo final.
    #[arg(long)]
    pub download_missing: bool,

    /// No guarda una copia del manifiesto junto a la salida.
    #[arg(long)]
    pub no_save_manifest: bool,

    /// Interno para pruebas: detente después de N piezas.
    #[arg(long, hide = true)]
    pub stop_after: Option<usize>,
}
