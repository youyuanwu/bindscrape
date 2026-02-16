//! End-to-end tests for Fcntl + Unistd file I/O bindings against real libc.

use bnd_posix::posix::fcntl;
use bnd_posix::posix::stat;
use bnd_posix::posix::unistd;

use std::ffi::CString;

/// Helper: create a temporary file path with a unique name.
fn tmp_path(name: &str) -> CString {
    CString::new(format!("/tmp/bnd_winmd_e2e_{name}_{}", std::process::id())).unwrap()
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn o_rdonly_is_zero() {
    assert_eq!(fcntl::O_RDONLY, 0);
}

#[test]
fn seek_constants() {
    assert_eq!(unistd::SEEK_SET, 0);
    assert_eq!(unistd::SEEK_CUR, 1);
    assert_eq!(unistd::SEEK_END, 2);
}

#[test]
fn access_mode_constants() {
    assert_eq!(unistd::R_OK, 4);
    assert_eq!(unistd::W_OK, 2);
    assert_eq!(unistd::X_OK, 1);
    assert_eq!(unistd::F_OK, 0);
}

// ---------------------------------------------------------------------------
// Simple syscall smoke tests
// ---------------------------------------------------------------------------

#[test]
fn getpid_returns_positive() {
    let pid = unsafe { unistd::getpid() };
    assert!(pid > 0, "getpid should return a positive value, got {pid}");
}

#[test]
fn getuid_returns_value() {
    let uid = unsafe { unistd::getuid() };
    let _ = uid;
}

// ---------------------------------------------------------------------------
// File I/O: creat, write, read, close, unlink
// ---------------------------------------------------------------------------

#[test]
fn creat_and_close() {
    let path = tmp_path("creat_close");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0, "creat failed with fd={fd}");
    let rc = unsafe { unistd::close(fd) };
    assert_eq!(rc, 0, "close failed");
    unsafe { unistd::unlink(path.as_ptr()) };
}

#[test]
fn write_then_read() {
    let path = tmp_path("write_read");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0, "creat failed");
    let data = b"hello bnd-winmd";
    let written = unsafe {
        unistd::write(
            fd,
            data.as_ptr() as *const core::ffi::c_void,
            data.len() as u64,
        )
    };
    assert_eq!(written, data.len() as i64, "write returned wrong count");
    unsafe { unistd::close(fd) };

    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    unsafe { unistd::close(fd) };

    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
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
    assert_eq!(st.st_size, data.len() as i64, "file size mismatch");

    unsafe { unistd::unlink(path.as_ptr()) };
}

// ---------------------------------------------------------------------------
// lseek
// ---------------------------------------------------------------------------

#[test]
fn lseek_returns_offset() {
    let path = tmp_path("lseek");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0);
    let data = b"abcdefghij"; // 10 bytes
    unsafe {
        unistd::write(
            fd,
            data.as_ptr() as *const core::ffi::c_void,
            data.len() as u64,
        )
    };

    let pos = unsafe { unistd::lseek(fd, 0, unistd::SEEK_CUR) };
    assert_eq!(pos, 10, "after writing 10 bytes, pos should be 10");

    let pos = unsafe { unistd::lseek(fd, 0, unistd::SEEK_SET) };
    assert_eq!(pos, 0, "SEEK_SET to 0");

    let pos = unsafe { unistd::lseek(fd, -3, unistd::SEEK_END) };
    assert_eq!(pos, 7, "SEEK_END - 3 on 10-byte file");

    unsafe { unistd::close(fd) };
    unsafe { unistd::unlink(path.as_ptr()) };
}

// ---------------------------------------------------------------------------
// access
// ---------------------------------------------------------------------------

#[test]
fn access_existing_file() {
    let path = tmp_path("access_exist");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0);
    unsafe { unistd::close(fd) };

    let rc = unsafe { unistd::access(path.as_ptr(), unistd::F_OK) };
    assert_eq!(rc, 0, "access F_OK should succeed for existing file");

    unsafe { unistd::unlink(path.as_ptr()) };
}

#[test]
fn access_nonexistent_file() {
    let path = CString::new("/tmp/bnd_winmd_e2e_no_such_file_ever").unwrap();
    let rc = unsafe { unistd::access(path.as_ptr(), unistd::F_OK) };
    assert_eq!(rc, -1, "access should fail for nonexistent file");
}

// ---------------------------------------------------------------------------
// unlink
// ---------------------------------------------------------------------------

#[test]
fn unlink_file() {
    let path = tmp_path("unlink");
    let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
    assert!(fd >= 0);
    unsafe { unistd::close(fd) };

    let rc = unsafe { unistd::unlink(path.as_ptr()) };
    assert_eq!(rc, 0, "unlink should succeed");

    let rc = unsafe { unistd::access(path.as_ptr(), unistd::F_OK) };
    assert_eq!(rc, -1, "file should be gone after unlink");
}
