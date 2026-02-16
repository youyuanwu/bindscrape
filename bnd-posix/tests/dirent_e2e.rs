//! End-to-end tests for Dirent bindings against real libc.

use bnd_posix::posix::dirent;

use std::ffi::CString;

#[test]
fn dt_type_constants() {
    assert_eq!(dirent::DT_UNKNOWN, 0);
    assert_eq!(dirent::DT_FIFO, 1);
    assert_eq!(dirent::DT_CHR, 2);
    assert_eq!(dirent::DT_DIR, 4);
    assert_eq!(dirent::DT_BLK, 6);
    assert_eq!(dirent::DT_REG, 8);
    assert_eq!(dirent::DT_LNK, 10);
    assert_eq!(dirent::DT_SOCK, 12);
}

#[test]
fn dirent_struct_size() {
    let size = core::mem::size_of::<dirent::dirent>();
    assert_eq!(
        size, 280,
        "struct dirent should be 280 bytes on x86_64 glibc"
    );
}

#[test]
fn opendir_readdir_closedir_roundtrip() {
    unsafe {
        let path = CString::new("/tmp").unwrap();
        let dir = dirent::opendir(path.as_ptr());
        assert!(!dir.is_null(), "opendir(\"/tmp\") should succeed");

        let entry = dirent::readdir(dir);
        assert!(!entry.is_null(), "readdir should return at least one entry");

        let d = &*entry;
        assert_ne!(d.d_ino, 0, "d_ino should be non-zero");
        assert!(d.d_type <= 14, "d_type should be a valid type");

        let ret = dirent::closedir(dir);
        assert_eq!(ret, 0, "closedir should succeed");
    }
}

#[test]
fn readdir_dot_entries() {
    unsafe {
        let path = CString::new("/tmp").unwrap();
        let dir = dirent::opendir(path.as_ptr());
        assert!(!dir.is_null());

        let mut found_dot = false;
        let mut found_dotdot = false;

        loop {
            let entry = dirent::readdir(dir);
            if entry.is_null() {
                break;
            }
            let d = &*entry;
            let name = std::ffi::CStr::from_ptr(d.d_name.as_ptr());
            if name.to_bytes() == b"." {
                found_dot = true;
                assert_eq!(d.d_type, dirent::DT_DIR as u8);
            } else if name.to_bytes() == b".." {
                found_dotdot = true;
                assert_eq!(d.d_type, dirent::DT_DIR as u8);
            }
        }

        assert!(found_dot, "should find '.' entry");
        assert!(found_dotdot, "should find '..' entry");

        dirent::closedir(dir);
    }
}

#[test]
fn dirfd_returns_valid_fd() {
    unsafe {
        let path = CString::new("/tmp").unwrap();
        let dir = dirent::opendir(path.as_ptr());
        assert!(!dir.is_null());

        let fd = dirent::dirfd(dir);
        assert!(fd >= 0, "dirfd should return a valid file descriptor");

        dirent::closedir(dir);
    }
}
