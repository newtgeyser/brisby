//! File chunking and reassembly

use crate::{error::Result, types::*, CHUNK_SIZE};
use std::io::{Read, Write};
use std::path::Path;

/// Chunk a file and compute its metadata
pub fn chunk_file(path: &Path) -> Result<(FileMetadata, Vec<Vec<u8>>)> {
    let file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut reader = std::io::BufReader::new(file);
    let mut chunks_data = Vec::new();
    let mut chunks_info = Vec::new();
    let mut content_hasher = blake3::Hasher::new();
    let mut index = 0u32;

    loop {
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let bytes_read = reader.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        buffer.truncate(bytes_read);

        // Hash the chunk
        let chunk_hash = blake3::hash(&buffer);
        let hash: ContentHash = *chunk_hash.as_bytes();

        // Feed the full file hasher with raw bytes
        content_hasher.update(&buffer);
        chunks_info.push(ChunkInfo {
            index,
            hash,
            size: bytes_read as u32,
        });
        chunks_data.push(buffer);

        index += 1;
    }

    // Compute file hash from the full file contents
    let content_hash: ContentHash = *content_hasher.finalize().as_bytes();

    let keywords = FileMetadata::extract_keywords(&filename);

    let metadata = FileMetadata {
        content_hash,
        filename,
        size: file_size,
        mime_type: detect_mime_type(path),
        chunks: chunks_info,
        keywords,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    Ok((metadata, chunks_data))
}

/// Reassemble chunks into a file
pub fn reassemble_file(
    chunks: &[Vec<u8>],
    metadata: &FileMetadata,
    output_path: &Path,
) -> Result<()> {
    // Verify chunk count
    if chunks.len() != metadata.chunks.len() {
        return Err(crate::error::Error::InvalidData(format!(
            "Expected {} chunks, got {}",
            metadata.chunks.len(),
            chunks.len()
        )));
    }

    // Verify each chunk hash
    for (i, (chunk_data, chunk_info)) in chunks.iter().zip(&metadata.chunks).enumerate() {
        let computed_hash = blake3::hash(chunk_data);
        if computed_hash.as_bytes() != &chunk_info.hash {
            return Err(crate::error::Error::HashMismatch {
                expected: hash_to_hex(&chunk_info.hash),
                actual: hex::encode(computed_hash.as_bytes()),
            });
        }

        if chunk_data.len() != chunk_info.size as usize {
            return Err(crate::error::Error::InvalidData(format!(
                "Chunk {} size mismatch: expected {}, got {}",
                i,
                chunk_info.size,
                chunk_data.len()
            )));
        }
    }

    // Write the file
    let mut file = std::fs::File::create(output_path)?;
    for chunk in chunks {
        file.write_all(chunk)?;
    }

    Ok(())
}

/// Verify a single chunk against its expected hash
pub fn verify_chunk(data: &[u8], expected_hash: &ContentHash) -> bool {
    let computed = blake3::hash(data);
    computed.as_bytes() == expected_hash
}

/// Simple MIME type detection based on file extension
fn detect_mime_type(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    let mime = match ext.as_str() {
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        _ => return None,
    };
    Some(mime.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_chunk_and_reassemble() {
        // Create a test file
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = vec![0x42u8; CHUNK_SIZE * 2 + 1000]; // 2.something chunks
        temp_file.write_all(&test_data).unwrap();

        // Chunk it
        let (metadata, chunks) = chunk_file(temp_file.path()).unwrap();

        assert_eq!(metadata.size, test_data.len() as u64);
        assert_eq!(chunks.len(), 3);
        assert_eq!(metadata.chunks.len(), 3);

        // Reassemble
        let output = NamedTempFile::new().unwrap();
        reassemble_file(&chunks, &metadata, output.path()).unwrap();

        // Verify
        let reassembled = std::fs::read(output.path()).unwrap();
        assert_eq!(reassembled, test_data);
    }

    #[test]
    fn test_content_hash_matches_raw_data() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"hash-me-now";
        temp_file.write_all(test_data).unwrap();

        let (metadata, _) = chunk_file(temp_file.path()).unwrap();

        let expected = blake3::hash(test_data);
        assert_eq!(metadata.content_hash, *expected.as_bytes());
    }

    #[test]
    fn test_extract_keywords() {
        let keywords = FileMetadata::extract_keywords("Big_Buck-Bunny.1080p.mkv");
        assert!(keywords.contains(&"big".to_string()));
        assert!(keywords.contains(&"buck".to_string()));
        assert!(keywords.contains(&"bunny".to_string()));
        assert!(keywords.contains(&"1080p".to_string()));
        assert!(keywords.contains(&"mkv".to_string()));
    }
}
