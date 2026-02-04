use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

/// Extract file extension from filename
pub fn extract_extension(filename: &str) -> Option<&str> {
    filename.rfind('.').map(|pos| &filename[pos..])
}

/// Generate a safe storage key using UUID and extension only
/// This avoids issues with non-ASCII characters in HTTP headers
pub fn generate_storage_key(prefix: &str, uuid: &str, filename: &str) -> String {
    let extension = extract_extension(filename).unwrap_or("");

    if prefix.is_empty() {
        format!("{}{}", uuid, extension)
    } else {
        format!("{}{}{}", prefix, uuid, extension)
    }
}

/// Encode filename for Content-Disposition header according to RFC 5987
/// Returns a header value like: attachment; filename*=UTF-8''%ED%95%9C%EA%B8%80.txt
pub fn encode_content_disposition(disposition_type: &str, filename: &str) -> String {
    if filename.is_ascii() {
        format!("{}; filename=\"{}\"", disposition_type, filename)
    } else {
        let encoded = utf8_percent_encode(filename, NON_ALPHANUMERIC).to_string();
        format!("{}; filename*=UTF-8''{}", disposition_type, encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_extension() {
        assert_eq!(extract_extension("test.txt"), Some(".txt"));
        assert_eq!(extract_extension("test.tar.gz"), Some(".gz"));
        assert_eq!(extract_extension("noextension"), None);
        assert_eq!(extract_extension("한글파일.pdf"), Some(".pdf"));
    }

    #[test]
    fn test_generate_storage_key() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";

        assert_eq!(
            generate_storage_key("", uuid, "test.txt"),
            "550e8400-e29b-41d4-a716-446655440000.txt"
        );

        assert_eq!(
            generate_storage_key("prefix/", uuid, "한글파일.pdf"),
            "prefix/550e8400-e29b-41d4-a716-446655440000.pdf"
        );
    }

    #[test]
    fn test_encode_content_disposition() {
        assert_eq!(
            encode_content_disposition("attachment", "test.txt"),
            "attachment; filename=\"test.txt\""
        );

        let result = encode_content_disposition("attachment", "한글.txt");
        assert!(result.starts_with("attachment; filename*=UTF-8''"));
    }
}