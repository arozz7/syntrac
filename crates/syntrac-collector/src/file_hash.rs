//! SHA-256 of a file, read in chunks so a stop request (service shutdown) is honoured between chunks instead of after a
//! half-gigabyte read.

use std::io::{self, Read};

use sha2::{Digest, Sha256};

const CHUNK: usize = 1024 * 1024;

#[derive(Debug)]
pub enum HashError {
    /// `should_stop` returned true between two chunks.
    Stopped,
    Io(io::Error),
}

/// Lower-case hex SHA-256 of everything `reader` yields. `should_stop` is asked before every chunk.
pub fn sha256_hex<R: Read + ?Sized>(
    reader: &mut R,
    mut should_stop: impl FnMut() -> bool,
) -> Result<String, HashError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        if should_stop() {
            return Err(HashError::Stopped);
        }
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(HashError::Io(error)),
        }
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}
