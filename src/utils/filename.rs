use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

pub fn extract_extension(filename: &str) -> Option<&str> {
    filename.rfind('.').map(|pos| &filename[pos..])
}

pub fn generate_storage_key(prefix: &str, uuid: &str, filename: &str) -> String {
    let extension = extract_extension(filename).unwrap_or("");

    if prefix.is_empty() {
        format!("{}{}", uuid, extension)
    } else {
        format!("{}{}{}", prefix, uuid, extension)
    }
}

pub const MAX_RELATIVE_PATH_LEN: usize = 1024;

pub fn sanitize_relative_path(raw: &str) -> String {
    if raw.contains('\0') {
        return String::new();
    }

    let normalized = raw.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');

    let mut segments: Vec<&str> = Vec::new();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return String::new(),
            other => segments.push(other),
        }
    }

    let result = segments.join("/");

    if result.len() > MAX_RELATIVE_PATH_LEN {
        return String::new();
    }

    result
}

pub fn normalize_relative_path(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let sanitized = sanitize_relative_path(raw);
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

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
    fn test_sanitize_relative_path() {
        assert_eq!(sanitize_relative_path("src/index.ts"), "src/index.ts");
        assert_eq!(sanitize_relative_path(""), "");
        assert_eq!(sanitize_relative_path("/etc/passwd"), "etc/passwd");
        assert_eq!(sanitize_relative_path("///a/b"), "a/b");
        assert_eq!(sanitize_relative_path("src\\components\\App.tsx"), "src/components/App.tsx");
        assert_eq!(sanitize_relative_path("./src//a.ts"), "src/a.ts");
        assert_eq!(sanitize_relative_path("a/./b/./c"), "a/b/c");
        assert_eq!(sanitize_relative_path("../../etc/passwd"), "");
        assert_eq!(sanitize_relative_path("a/../b"), "");
        assert_eq!(sanitize_relative_path("a/b/.."), "");
        assert_eq!(sanitize_relative_path("a/\0b"), "");
        assert_eq!(sanitize_relative_path("폴더/파일.txt"), "폴더/파일.txt");
        let too_long = "a/".repeat(600);
        assert_eq!(sanitize_relative_path(&too_long), "");
    }

    #[test]
    fn test_normalize_relative_path() {
        assert_eq!(normalize_relative_path(None), None);
        assert_eq!(normalize_relative_path(Some("")), None);
        assert_eq!(normalize_relative_path(Some("/")), None);
        assert_eq!(normalize_relative_path(Some("../x")), None);
        assert_eq!(
            normalize_relative_path(Some("src/index.ts")),
            Some("src/index.ts".to_string())
        );
        assert_eq!(
            normalize_relative_path(Some("\\a\\b")),
            Some("a/b".to_string())
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