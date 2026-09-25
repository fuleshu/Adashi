//! Checked checkout paths and atomic output publication; never follow reparse points.
use std::{fs, io::Write, path::{Component, Path, PathBuf}};

pub(super) fn relative(value: &str) -> Result<String, String> {
    let value = value.replace('\\', "/");
    for part in value.split('/') {
        let stem=part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if ["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"].contains(&stem.as_str()) { return Err("Reserved device name in output directory".into()); }
    }
    if value.is_empty() || value.split('/').any(|p| p.is_empty() || p == "." || p == ".." || p.starts_with('.') || p.ends_with([' ', '.']) || p.contains([':', '*', '?', '<', '>', '|', '"']) || p.chars().any(char::is_control)) || Path::new(&value).components().any(|p| !matches!(p,Component::Normal(_))) {
        return Err("Use a project-relative directory with ordinary path components.".into());
    }
    Ok(value)
}

pub(super) fn checked(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in relative.replace('\\',"/").split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') { return Err("Unsafe projection path".into()); }
        // Reject case aliases even on a case-sensitive filesystem, preserving portability.
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.eq_ignore_ascii_case(part) && name != part { return Err(format!("Projection path case collision: {relative}")); }
            }
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                #[cfg(windows)]
                let reparse = { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 };
                #[cfg(not(windows))]
                let reparse = false;
                if meta.file_type().is_symlink() || reparse { return Err(format!("Projection path contains a symlink or junction: {relative}")); }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

pub(super) fn write(root: &Path, relative: &str, bytes: &[u8]) -> Result<bool,String> {
    let path = checked(root,relative)?;
    match fs::read(&path) {
        Ok(current) if current == bytes => return Ok(false),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {},
    }
    let parent = path.parent().ok_or("Missing projection parent")?;
    fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    checked(root,relative)?;
    let mut temp = tempfile::Builder::new().prefix(".adashi-output-").tempfile_in(parent).map_err(|e|e.to_string())?;
    temp.write_all(bytes).map_err(|e|e.to_string())?;
    temp.as_file().sync_all().map_err(|e|e.to_string())?;
    temp.persist(&path).map_err(|e|e.to_string())?;
    Ok(true)
}

pub(super) fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest,Sha256};
    format!("{:x}",Sha256::digest(bytes))
}
