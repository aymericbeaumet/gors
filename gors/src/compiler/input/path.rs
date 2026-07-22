use std::fmt;

/// Structural reason a logical source path is not canonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalPathIssue {
    Empty,
    Absolute,
    WindowsDrivePrefix,
    Backslash,
    EmptyComponent,
    CurrentDirectoryComponent,
    ParentDirectoryComponent,
}

impl fmt::Display for LogicalPathIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "path is empty",
            Self::Absolute => "path must be relative",
            Self::WindowsDrivePrefix => "path must not contain a Windows drive prefix",
            Self::Backslash => "path must use forward slashes",
            Self::EmptyComponent => "path contains an empty component",
            Self::CurrentDirectoryComponent => "path contains a '.' component",
            Self::ParentDirectoryComponent => "path contains a '..' component",
        };
        formatter.write_str(message)
    }
}

pub(super) fn validate_logical_path(path: &str) -> Result<(), LogicalPathIssue> {
    if path.is_empty() {
        return Err(LogicalPathIssue::Empty);
    }
    if path.starts_with('/') {
        return Err(LogicalPathIssue::Absolute);
    }
    if has_windows_drive_prefix(path) {
        return Err(LogicalPathIssue::WindowsDrivePrefix);
    }
    if path.contains('\\') {
        return Err(LogicalPathIssue::Backslash);
    }
    for component in path.split('/') {
        match component {
            "" => return Err(LogicalPathIssue::EmptyComponent),
            "." => return Err(LogicalPathIssue::CurrentDirectoryComponent),
            ".." => return Err(LogicalPathIssue::ParentDirectoryComponent),
            _ => {}
        }
    }
    Ok(())
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    matches!(bytes, [drive, b':', ..] if drive.is_ascii_alphabetic())
}
