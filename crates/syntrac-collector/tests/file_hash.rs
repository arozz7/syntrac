//! Hashing a file in chunks, stoppable between chunks.

use std::io::{self, Read};

use syntrac_collector::file_hash::{sha256_hex, HashError};

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn known_answers() {
    assert_eq!(sha256_hex(&mut &b"abc"[..], || false).unwrap(), ABC);
    assert_eq!(sha256_hex(&mut &b""[..], || false).unwrap(), EMPTY);
}

#[test]
fn a_file_larger_than_one_chunk_hashes_the_same_as_in_one_piece() {
    // 3 MB of a repeating pattern: the answer of the one-shot reference is below.
    let data: Vec<u8> = (0..3 * 1024 * 1024).map(|i: usize| (i % 251) as u8).collect();
    let chunked = sha256_hex(&mut data.as_slice(), || false).unwrap();
    let one_piece = {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(&data);
        digest.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };

    assert_eq!(chunked, one_piece);
}

#[test]
fn a_stop_request_is_honoured_between_chunks() {
    let data = vec![7_u8; 5 * 1024 * 1024];
    let mut chunks_seen = 0;

    let result = sha256_hex(&mut data.as_slice(), || {
        chunks_seen += 1;
        chunks_seen > 2
    });

    assert!(matches!(result, Err(HashError::Stopped)));
    assert_eq!(chunks_seen, 3, "stopped right after the third check, not at the end of the file");
}

struct Broken;
impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
    }
}

#[test]
fn a_read_error_is_reported_not_hidden() {
    assert!(matches!(sha256_hex(&mut Broken, || false), Err(HashError::Io(_))));
}
