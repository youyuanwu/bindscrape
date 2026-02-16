//! End-to-end tests for Stat bindings against real libc.

use bnd_posix::posix::fcntl;
use bnd_posix::posix::stat;
use bnd_posix::posix::unistd;

use std::ffi::CString;

fn tmp_path(name: &str) -> CString {
    CString::new(format!("/tmp/bnd_winmd_e2e_{name}_{}", std::process::id())).unwrap()
}

#[test]
fn stat_file_size() {
    let path = tmp_path("stat_size");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0);
    let data = b"0123456789";
    unsafe {
        unistd::write(
            fd,
            data.as_ptr() as *const core::ffi::c_void,
            data.len() as u64,
        )
    };
    unsafe { unistd::close(fd) };

    let mut st = stat::stat::default();
    let rc = unsafe { stat::stat(path.as_ptr(), &mut st as *mut _ as *const _) };
    assert_eq!(rc, 0, "stat failed");
    assert_eq!(st.st_size, 10);

    unsafe { unistd::unlink(path.as_ptr()) };
}

#[test]
fn stat_is_regular_file() {
    let path = tmp_path("stat_reg");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0);
    unsafe { unistd::close(fd) };

    let mut st = stat::stat::default();
    let rc = unsafe { stat::stat(path.as_ptr(), &mut st as *mut _ as *const _) };
    assert_eq!(rc, 0);
    assert_eq!(
        st.st_mode & 0o170000,
        0o100000,
        "expected S_IFREG, got mode {:#o}",
        st.st_mode
    );

    unsafe { unistd::unlink(path.as_ptr()) };
}

#[test]
fn stat_struct_size() {
    assert_eq!(
        std::mem::size_of::<stat::stat>(),
        144,
        "struct stat should be 144 bytes on x86_64 Linux"
    );
}

#[test]
fn timespec_struct_size() {
    assert_eq!(
        std::mem::size_of::<stat::timespec>(),
        16,
        "struct timespec should be 16 bytes"
    );
}
