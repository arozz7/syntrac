//! Human-readable names for processes.

/// The file name of the image path, or a sensible label when there is no path (the System process has none).
pub fn display_name(image_path: &str, pid: u32) -> String {
    let path = image_path.trim();
    let file_name = path.rsplit(['\\', '/']).next().unwrap_or_default();
    if !file_name.is_empty() {
        return file_name.to_owned();
    }
    match pid {
        0 => "Idle".to_owned(),
        4 => "System".to_owned(),
        other => format!("pid {other}"),
    }
}
