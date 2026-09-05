//! Path parsing and normalization utilities.

/// Normalize a path (resolve `.` and `..` components).
pub fn normalize(path: &str) -> &str {
    // Stub: real implementation walks components
    path
}

/// Split a path into parent and filename components.
pub fn split(path: &str) -> (&str, &str) {
    if let Some(pos) = path.rfind('/') {
        (&path[..pos], &path[pos + 1..])
    } else {
        (".", path)
    }
}

/// Check if a path is absolute.
pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}
