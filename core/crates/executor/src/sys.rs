//! Thin wrappers over the few system calls the executor needs, all relative
//! to open folder handles, so that no symlink is ever followed and a folder
//! cannot be swapped for another under a path we already checked.
//!
//! Every folder is opened with `O_NOFOLLOW | O_DIRECTORY`: opening fails if
//! the name is a symlink or not a folder. Moves use the "never replace"
//! variant of `renameat`, so a move can never overwrite anything.

use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Flags for opening a folder: read only, must be a folder, never a symlink.
const DIR_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// What kind of thing a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A folder.
    Dir,
    /// A symlink.
    Symlink,
    /// A plain file, or anything else that is not a folder or symlink.
    Other,
}

/// The parts of `stat` the executor compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    /// Device id (`st_dev`).
    pub dev: u64,
    /// Inode number (`st_ino`).
    pub ino: u64,
    /// Folder, symlink or other.
    pub kind: Kind,
}

impl Stat {
    /// Converts a raw `stat`. The field types differ between macOS and Linux,
    /// so the casts are needed on one and redundant on the other.
    #[allow(clippy::unnecessary_cast)]
    fn from_raw(st: &libc::stat) -> Stat {
        let fmt = st.st_mode & libc::S_IFMT;
        let kind = if fmt == libc::S_IFDIR {
            Kind::Dir
        } else if fmt == libc::S_IFLNK {
            Kind::Symlink
        } else {
            Kind::Other
        };
        Stat {
            dev: st.st_dev as u64,
            ino: st.st_ino as u64,
            kind,
        }
    }
}

/// Makes a C string from a single name. Refuses a name with a `/` or a NUL
/// byte, or `.`/`..`, so a name can never reach outside its folder.
fn name_cstr(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    CString::new(bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// Turns a `-1` return into the current OS error.
fn check(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// Opens the folder at an absolute path. Fails if its last part is a symlink.
pub fn open_dir(path: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `c` is a valid NUL-terminated string for the whole call.
    let fd = check(unsafe { libc::open(c.as_ptr(), DIR_FLAGS) })?;
    // SAFETY: `open` returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Opens the folder `name` inside `dir`. Fails if `name` is a symlink.
pub fn open_dir_at(dir: &OwnedFd, name: &OsStr) -> io::Result<OwnedFd> {
    let c = name_cstr(name)?;
    // SAFETY: `dir` is an open descriptor and `c` a valid C string.
    let fd = check(unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), DIR_FLAGS) })?;
    // SAFETY: `openat` returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// `fstat` of an open descriptor.
pub fn fstat(fd: &OwnedFd) -> io::Result<Stat> {
    // SAFETY: an all-zero `stat` is a valid value of this plain C struct.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is open and `st` is a valid, writable `stat`.
    check(unsafe { libc::fstat(fd.as_raw_fd(), &mut st) })?;
    Ok(Stat::from_raw(&st))
}

/// `lstat` of `name` inside `dir`: never follows a symlink.
pub fn stat_at(dir: &OwnedFd, name: &OsStr) -> io::Result<Stat> {
    let c = name_cstr(name)?;
    // SAFETY: an all-zero `stat` is a valid value of this plain C struct.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `dir` is open, `c` is a valid C string, `st` is writable.
    check(unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            c.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    Ok(Stat::from_raw(&st))
}

/// Makes the folder `name` inside `dir`. Fails if anything has that name.
pub fn mkdir_at(dir: &OwnedFd, name: &OsStr) -> io::Result<()> {
    let c = name_cstr(name)?;
    // SAFETY: `dir` is open and `c` is a valid C string.
    check(unsafe { libc::mkdirat(dir.as_raw_fd(), c.as_ptr(), 0o700) })?;
    Ok(())
}

/// Moves `from` in `from_dir` to `to` in `to_dir`. Fails with
/// `AlreadyExists` instead of replacing anything already called `to`.
/// A symlink named `from` is moved as itself, never followed.
pub fn rename_no_replace(
    from_dir: &OwnedFd,
    from: &OsStr,
    to_dir: &OwnedFd,
    to: &OsStr,
) -> io::Result<()> {
    let from = name_cstr(from)?;
    let to = name_cstr(to)?;
    // SAFETY: both descriptors are open and both names are valid C strings.
    check(unsafe { rename_excl(from_dir.as_raw_fd(), &from, to_dir.as_raw_fd(), &to) })?;
    Ok(())
}

/// macOS: `renameatx_np` with `RENAME_EXCL`.
///
/// # Safety
/// Both descriptors must be open.
#[cfg(target_os = "macos")]
unsafe fn rename_excl(
    from_dir: libc::c_int,
    from: &CString,
    to_dir: libc::c_int,
    to: &CString,
) -> libc::c_int {
    libc::renameatx_np(
        from_dir,
        from.as_ptr(),
        to_dir,
        to.as_ptr(),
        libc::RENAME_EXCL,
    )
}

/// Linux (for tests on a second file system): `renameat2` with
/// `RENAME_NOREPLACE`.
///
/// # Safety
/// Both descriptors must be open.
#[cfg(target_os = "linux")]
unsafe fn rename_excl(
    from_dir: libc::c_int,
    from: &CString,
    to_dir: libc::c_int,
    to: &CString,
) -> libc::c_int {
    libc::renameat2(
        from_dir,
        from.as_ptr(),
        to_dir,
        to.as_ptr(),
        libc::RENAME_NOREPLACE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_leave_the_folder_are_refused() {
        for bad in ["", ".", "..", "a/b", "/abs"] {
            assert!(name_cstr(OsStr::new(bad)).is_err(), "{bad:?}");
        }
        assert!(name_cstr(OsStr::new("a\0b")).is_err());
        assert!(name_cstr(OsStr::new("Caches")).is_ok());
    }

    #[test]
    fn open_dir_refuses_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        std::os::unix::fs::symlink("real", dir.path().join("link")).unwrap();
        let root = open_dir(dir.path()).unwrap();
        assert!(open_dir_at(&root, OsStr::new("real")).is_ok());
        assert!(open_dir_at(&root, OsStr::new("link")).is_err());
        assert!(open_dir(&dir.path().join("link")).is_err());
        assert_eq!(
            stat_at(&root, OsStr::new("link")).unwrap().kind,
            Kind::Symlink
        );
    }

    #[test]
    fn rename_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"a").unwrap();
        std::fs::write(dir.path().join("b"), b"b").unwrap();
        let root = open_dir(dir.path()).unwrap();
        let err = rename_no_replace(&root, OsStr::new("a"), &root, OsStr::new("b")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(dir.path().join("b")).unwrap(), b"b");
        rename_no_replace(&root, OsStr::new("a"), &root, OsStr::new("c")).unwrap();
        assert_eq!(std::fs::read(dir.path().join("c")).unwrap(), b"a");
    }
}
