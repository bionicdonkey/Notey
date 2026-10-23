use super::fs_util::atomic_write;
use crate::core::encoding::{self, Decoded, Encoding};
use crate::core::line_ending::LineEnding;
use crate::core::session::{sha256_hex, DiskState};
use std::fs;
use std::io;
use std::path::Path;

/// Size limits from the spec (§3.6). Constants live here so settings can override them.
#[derive(Debug, Clone, Copy)]
pub struct SizeLimits {
    pub no_highlight: u64,
    pub confirm: u64,
    pub refuse: u64,
}

impl Default for SizeLimits {
    fn default() -> Self {
        Self { no_highlight: 5 << 20, confirm: 50 << 20, refuse: 200 << 20 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeTier {
    Normal,
    /// Highlighting, minimap and current-line highlight off.
    Large,
    /// As `Large`, after an "Open anyway?" confirmation; match counting off.
    VeryLarge,
    Refuse,
}

impl SizeLimits {
    pub fn classify(&self, size: u64) -> SizeTier {
        if size > self.refuse {
            SizeTier::Refuse
        } else if size > self.confirm {
            SizeTier::VeryLarge
        } else if size > self.no_highlight {
            SizeTier::Large
        } else {
            SizeTier::Normal
        }
    }
}

/// A NUL byte near the start that is not explained by UTF-16/32.
pub fn looks_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    head.contains(&0) && !decode_head_is_wide(head)
}

fn decode_head_is_wide(head: &[u8]) -> bool {
    matches!(
        encoding::decode(head, None).encoding,
        Encoding::Utf16Le | Encoding::Utf16Be | Encoding::Utf32Le | Encoding::Utf32Be
    )
}

/// Whether the current user may write the file (a root-owned 0644 file is not writable,
/// which `Permissions::readonly` cannot tell).
pub fn can_write(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        match std::ffi::CString::new(path.as_os_str().as_bytes()) {
            Ok(c) => unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 },
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        fs::metadata(path).map_or(false, |m| !m.permissions().readonly())
    }
}

pub fn disk_state_of(path: &Path, bytes: Option<&[u8]>) -> io::Result<DiskState> {
    let meta = fs::metadata(path)?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64);
    let sha256 = match bytes {
        Some(b) => sha256_hex(b),
        None => sha256_hex(&fs::read(path)?),
    };
    Ok(DiskState { mtime_ns, size: meta.len(), sha256 })
}

#[derive(Debug)]
pub struct LoadedFile {
    pub decoded: Decoded,
    pub disk: DiskState,
    pub read_only: bool,
    pub binary: bool,
}

/// Reads and decodes a file. `forced` re-reads with a user-chosen encoding.
pub fn read_file(path: &Path, forced: Option<Encoding>) -> io::Result<LoadedFile> {
    let bytes = fs::read(path)?;
    let disk = disk_state_of(path, Some(&bytes))?;
    let binary = looks_binary(&bytes);
    let read_only = !can_write(path) || binary;
    Ok(LoadedFile { decoded: encoding::decode(&bytes, forced), disk, read_only, binary })
}

/// Encodes and writes atomically, keeping the target's permissions. A symlink is
/// followed so the link survives. When the directory does not allow a temp file but
/// the file itself is writable, the file is rewritten in place. Callers run
/// `encoding::check_convertible` first when they want to warn about lossy output.
pub fn save_file(
    path: &Path,
    text: &str,
    enc: Encoding,
    bom: bool,
    ending: LineEnding,
) -> io::Result<DiskState> {
    let encoded = encoding::encode(text, enc, bom, ending);
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&target).ok().map(|m| m.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let mode = None;
    match atomic_write(&target, &encoded.bytes, mode) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && target.exists() && can_write(&target) => {
            fs::write(&target, &encoded.bytes)?;
        }
        Err(e) => return Err(e),
    }
    disk_state_of(&target, Some(&encoded.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_tiers() {
        let l = SizeLimits::default();
        assert_eq!(l.classify(1), SizeTier::Normal);
        assert_eq!(l.classify(5 << 20), SizeTier::Normal);
        assert_eq!(l.classify((5 << 20) + 1), SizeTier::Large);
        assert_eq!(l.classify((50 << 20) + 1), SizeTier::VeryLarge);
        assert_eq!(l.classify((200 << 20) + 1), SizeTier::Refuse);
    }

    #[test]
    fn binary_guard() {
        assert!(looks_binary(b"\x7fELF\0\0\0\x01"));
        assert!(!looks_binary(b"plain text"));
        let utf16 = encoding::encode("hello there", Encoding::Utf16Le, true, LineEnding::Lf).bytes;
        assert!(!looks_binary(&utf16));
    }

    #[test]
    fn save_then_read_round_trips_and_reports_disk_state() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.txt");
        let enc = Encoding::from_name("windows-1251").unwrap();
        let state = save_file(&p, "Привет\nмир\n", enc, false, LineEnding::Crlf).unwrap();
        let loaded = read_file(&p, Some(enc)).unwrap();
        assert_eq!(loaded.decoded.text, "Привет\nмир\n");
        assert_eq!(loaded.decoded.line_ending, LineEnding::Crlf);
        assert_eq!(loaded.disk.sha256, state.sha256);
        assert_eq!(loaded.disk.size, state.size);
        assert!(!loaded.read_only);
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("script.sh");
        fs::write(&p, "old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        save_file(&p, "new", Encoding::Utf8, false, LineEnding::Lf).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.txt");
        let link = d.path().join("link.txt");
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save_file(&link, "new", Encoding::Utf8, false, LineEnding::Lf).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn writable_file_in_a_read_only_directory_is_rewritten_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.txt");
        fs::write(&p, "old").unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let r = save_file(&p, "new", Encoding::Utf8, false, LineEnding::Lf);
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o755)).unwrap();
        r.unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn write_permission_check_uses_access() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        fs::write(&p, "x").unwrap();
        assert!(can_write(&p));
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
        assert!(!can_write(&p));
        assert!(read_file(&p, None).unwrap().read_only);
    }

    #[test]
    fn unchanged_open_save_is_byte_identical() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.txt");
        let original = b"\xEF\xBB\xBFline one\r\nline two\r\nno trailing newline".to_vec();
        fs::write(&p, &original).unwrap();
        let l = read_file(&p, None).unwrap();
        save_file(&p, &l.decoded.text, l.decoded.encoding, l.decoded.bom, l.decoded.line_ending).unwrap();
        assert_eq!(fs::read(&p).unwrap(), original);
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(read_file(Path::new("/definitely/not/here"), None).is_err());
    }
}
