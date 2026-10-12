use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSpace {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[cfg(unix)]
pub fn probe_disk(path: &Path) -> Option<DiskSpace> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let block_size = stat.f_frsize as u64;
    let free_bytes = block_size.saturating_mul(stat.f_bavail as u64);
    let total_bytes = block_size.saturating_mul(stat.f_blocks as u64);
    Some(DiskSpace {
        free_bytes,
        total_bytes,
    })
}

#[cfg(not(unix))]
pub fn probe_disk(_path: &Path) -> Option<DiskSpace> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_real_tmp_directory() {
        let dir = crate::testscratch::dir("disk");
        let space = probe_disk(dir.path()).expect("statvfs should succeed on a real tmp dir");
        assert!(space.total_bytes > 0);
        assert!(space.free_bytes <= space.total_bytes);
    }

    #[test]
    fn nonexistent_path_returns_none_without_panic() {
        let path = Path::new("/no/such/path/pito-probe-test-fixture");
        assert_eq!(probe_disk(path), None);
    }
}
