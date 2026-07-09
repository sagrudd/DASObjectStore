use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub const SHA256_ALGORITHM: &str = "sha256";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HashWriteReport {
    pub bytes_written: u64,
    pub content_hash: String,
}

pub(crate) fn copy_and_hash_with_controlled_progress(
    reader: &mut impl Read,
    writer: &mut impl Write,
    mut progress: impl FnMut(u64) -> Result<(), std::io::Error>,
) -> Result<HashWriteReport, std::io::Error> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_written = 0_u64;

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        writer.write_all(chunk)?;
        hasher.update(chunk);
        bytes_written += read as u64;
        progress(bytes_written)?;
    }

    Ok(HashWriteReport {
        bytes_written,
        content_hash: encode_hex(&hasher.finalize()),
    })
}

pub fn hash_file_sha256(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    hash_file_sha256_with_progress(path, |_| Ok(()))
}

pub fn hash_file_sha256_with_progress(
    path: impl AsRef<Path>,
    mut progress: impl FnMut(u64) -> Result<(), std::io::Error>,
) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut sink = std::io::sink();
    copy_and_hash_with_controlled_progress(&mut file, &mut sink, |bytes| progress(bytes))
        .map(|report| report.content_hash)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
