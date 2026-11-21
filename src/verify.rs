use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub fn hash_range(path: &Path, offset: u64, length: u64, buffer: &mut [u8]) -> io::Result<String> {
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
        remaining -= read as u64;
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn hex_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
