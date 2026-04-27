use std::{
    fs::File,
    io::{
        BufReader,
        Read,
    },
    path::Path,
};

use sha2::{
    Digest,
    Sha256,
};

use crate::error::IoContext;

pub(crate) fn hash_file<P: AsRef<Path>>(path: P) -> crate::core::Result<String> {
    let path = path.as_ref();
    let file = File::open(path).io_err(format!("opening file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192]; // 8kb buffer

    loop {
        let count = reader
            .read(&mut buffer)
            .io_err(format!("reading file: {}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn hash_string(content: &str) -> String {
        hex::encode(Sha256::digest(content))
    }

    #[test]
    fn same_content_produces_same_hash() {
        let content = b"hello world";
        let mut f1 = NamedTempFile::new().unwrap();
        let mut f2 = NamedTempFile::new().unwrap();
        f1.write_all(content).unwrap();
        f2.write_all(content).unwrap();
        assert_eq!(
            hash_file(&f1.path().to_path_buf()).unwrap(),
            hash_file(&f2.path().to_path_buf()).unwrap()
        );
    }

    #[test]
    fn different_content_produces_different_hash() {
        let mut f1 = NamedTempFile::new().unwrap();
        let mut f2 = NamedTempFile::new().unwrap();
        f1.write_all(b"hello").unwrap();
        f2.write_all(b"world").unwrap();
        assert_ne!(
            hash_file(&f1.path().to_path_buf()).unwrap(),
            hash_file(&f2.path().to_path_buf()).unwrap()
        );
    }

    #[test]
    fn hash_string_matches_hash_file_for_same_content() {
        let content = "test content";
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        assert_eq!(hash_string(content), hash_file(&f.path().to_path_buf()).unwrap());
    }

    #[test]
    fn hash_is_lowercase_hex() {
        let h = hash_string("any content");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(h.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }
}
