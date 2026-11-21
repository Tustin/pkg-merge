use crate::error::{Error, Result};
use crate::util;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

pub struct Log {
    file: Mutex<Option<File>>,
    start: Instant,
    verbose: bool,
}

impl Log {
    pub fn new(path: Option<&Path>, verbose: bool) -> Result<Self> {
        let file = match path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).map_err(|e| {
                            Error::io_path("no se pudo crear la carpeta del log", parent, e)
                        })?;
                    }
                }
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| Error::io_path("no se pudo abrir el log", path, e))?;
                Some(file)
            }
            None => None,
        };
        Ok(Self {
            file: Mutex::new(file),
            start: Instant::now(),
            verbose,
        })
    }

    fn write(&self, level: &str, message: &str, to_stderr: bool) {
        if to_stderr {
            eprintln!("[{level}] {message}");
        }
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let elapsed = util::format_duration(self.start.elapsed());
                let _ = writeln!(file, "[{elapsed:>12}] [{level}] {message}");
                let _ = file.flush();
            }
        }
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.write("info", message.as_ref(), true);
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        self.write("warn", message.as_ref(), true);
    }

    pub fn error(&self, message: impl AsRef<str>) {
        self.write("error", message.as_ref(), true);
    }

    pub fn detail(&self, message: impl AsRef<str>) {
        let has_file = self
            .file
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false);
        if self.verbose || has_file {
            self.write("detail", message.as_ref(), self.verbose);
        }
    }
}
