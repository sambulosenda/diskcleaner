//! Lists the programs running now, so an item is skipped while the app that
//! owns it is open. Read only: it asks the system for process paths and
//! never signals, starts or changes a process.

use std::io;
use std::path::PathBuf;

/// The executable path of every process this user can see.
///
/// A process whose path can't be read (it ended meanwhile, or belongs to
/// another user and is hidden) is left out. Fails only if the list of
/// processes itself can't be read.
#[cfg(target_os = "macos")]
pub fn running_programs() -> io::Result<Vec<PathBuf>> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: a null buffer with size 0 asks only for the number of pids.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    // Room for processes started between the two calls.
    let capacity = usize::try_from(count).unwrap_or(0).saturating_add(256);
    let mut pids: Vec<libc::pid_t> = vec![0; capacity];
    let bytes = capacity.saturating_mul(std::mem::size_of::<libc::pid_t>());
    let bytes = libc::c_int::try_from(bytes).map_err(|_| io::Error::other("too many processes"))?;
    // SAFETY: the buffer holds `capacity` pids, which is `bytes` bytes.
    let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if got <= 0 {
        return Err(io::Error::last_os_error());
    }
    pids.truncate(usize::try_from(got).unwrap_or(0).min(capacity));
    let mut programs = Vec::with_capacity(pids.len());
    let size = usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).unwrap_or(4096);
    let mut buf = vec![0u8; size];
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        let len = u32::try_from(buf.len()).unwrap_or(4096);
        // SAFETY: `buf` is `len` bytes long and outlives the call.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), len) };
        if let Ok(n) = usize::try_from(n) {
            if n > 0 && n <= buf.len() {
                programs.push(PathBuf::from(OsStr::from_bytes(&buf[..n])));
            }
        }
    }
    Ok(programs)
}

/// The executable path of every process this user can see (Linux, used by
/// tests and CI). Falls back to the process name when the path is hidden.
#[cfg(not(target_os = "macos"))]
pub fn running_programs() -> io::Result<Vec<PathBuf>> {
    let mut programs = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        if let Ok(exe) = std::fs::read_link(dir.join("exe")) {
            programs.push(exe);
        } else if let Ok(comm) = std::fs::read_to_string(dir.join("comm")) {
            programs.push(PathBuf::from(comm.trim_end()));
        }
    }
    Ok(programs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_at_least_this_test() {
        let programs = running_programs().unwrap();
        assert!(!programs.is_empty());
    }
}
