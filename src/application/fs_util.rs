use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Writes `bytes` to a sibling temp file, fsyncs, and renames it over `path`, so a
/// crash leaves either the old or the new content, never a mix. `mode` applies to
/// new files only on Unix; pass the existing file's mode to preserve it.
pub fn atomic_write(path: &Path, bytes: &[u8], mode: Option<u32>) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.notey-tmp"));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_content_and_leaves_no_temp_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.txt");
        atomic_write(&p, b"one", None).unwrap();
        atomic_write(&p, b"two", None).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn failure_leaves_the_original() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("dir");
        fs::create_dir(&p).unwrap(); // renaming a file over a directory fails
        assert!(atomic_write(&p, b"x", None).is_err());
        assert!(p.is_dir());
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1, "temp file cleaned up");
    }
}
