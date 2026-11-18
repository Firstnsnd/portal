//! Child-process name fallback — macOS: `proc_pidpath` (libproc).

/// Resolve a pid's executable file name via `proc_pidpath`, which reads the
/// kernel's exec path directly and answers even when `ps` cannot (e.g. a
/// stale or zombie entry in the process table).
pub fn pid_executable_name(pid: i32) -> Option<String> {
    use std::path::PathBuf;
    unsafe {
        let mut path: Vec<u8> = vec![0; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        if libc::proc_pidpath(
            pid,
            path.as_mut_ptr() as *mut libc::c_void,
            path.len() as u32,
        ) > 0 {
            let null_pos = path.iter().position(|&b| b == 0).unwrap_or(path.len());
            let path_str = std::str::from_utf8(&path[..null_pos]).ok()?;
            PathBuf::from(path_str)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}
