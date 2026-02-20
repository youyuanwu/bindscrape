//! End-to-end tests for stdio bindings against real libc.

use bnd_posix::posix::stdio;

#[test]
fn stdio_constants() {
    assert_eq!(stdio::BUFSIZ, 8192);
    assert_eq!(stdio::SEEK_SET, 0);
    assert_eq!(stdio::SEEK_CUR, 1);
    assert_eq!(stdio::SEEK_END, 2);
    assert_eq!(stdio::L_tmpnam, 20);
    assert_eq!(stdio::TMP_MAX, 238328);
    assert_eq!(stdio::FOPEN_MAX, 16);
    assert_eq!(stdio::FILENAME_MAX, 4096);
    assert_eq!(stdio::_IOFBF, 0);
    assert_eq!(stdio::_IOLBF, 1);
    assert_eq!(stdio::_IONBF, 2);
}

#[test]
fn fopen_fclose() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_fopen".as_ptr();
        let mode = c"w".as_ptr();
        let f = stdio::fopen(path, mode);
        assert!(!f.is_null(), "fopen should return a non-null FILE*");
        let ret = stdio::fclose(f);
        assert_eq!(ret, 0, "fclose should succeed");
        // Clean up
        stdio::remove(path);
    }
}

#[test]
fn fwrite_fread_roundtrip() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_rw".as_ptr();
        let f = stdio::fopen(path, c"w+".as_ptr());
        assert!(!f.is_null());

        let data = b"hello stdio";
        let written = stdio::fwrite(
            data.as_ptr() as *const core::ffi::c_void,
            1,
            data.len() as u64,
            f,
        );
        assert_eq!(written, data.len() as u64);

        stdio::rewind(f);

        let mut buf = [0u8; 32];
        let read = stdio::fread(
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            1,
            data.len() as u64,
            f,
        );
        assert_eq!(read, data.len() as u64);
        assert_eq!(&buf[..data.len()], data);

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn fgets_fputs() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_fgets".as_ptr();
        let f = stdio::fopen(path, c"w+".as_ptr());
        assert!(!f.is_null());

        let msg = c"test line\n".as_ptr();
        let ret = stdio::fputs(msg, f);
        assert!(ret >= 0, "fputs should succeed");

        stdio::rewind(f);

        let mut buf = [0i8; 64];
        let result = stdio::fgets(buf.as_mut_ptr(), buf.len() as i32, f);
        assert!(!result.is_null(), "fgets should return the buffer pointer");
        assert_eq!(&buf[..10], b"test line\n".map(|b| b as i8));

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn fseek_ftell() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_seek".as_ptr();
        let f = stdio::fopen(path, c"w+".as_ptr());
        assert!(!f.is_null());

        // Write some data so the file has content
        let data = b"0123456789";
        stdio::fwrite(data.as_ptr() as *const core::ffi::c_void, 1, 10, f);

        // Seek to offset 5
        let ret = stdio::fseek(f, 5, stdio::SEEK_SET);
        assert_eq!(ret, 0);

        let pos = stdio::ftell(f);
        assert_eq!(pos, 5, "ftell should return the seek position");

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn fgetc_fputc() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_putc".as_ptr();
        let f = stdio::fopen(path, c"w+".as_ptr());
        assert!(!f.is_null());

        stdio::fputc(b'A' as i32, f);
        stdio::fputc(b'B' as i32, f);
        stdio::rewind(f);

        assert_eq!(stdio::fgetc(f), b'A' as i32);
        assert_eq!(stdio::fgetc(f), b'B' as i32);

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn fileno_returns_valid_fd() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_fileno".as_ptr();
        let f = stdio::fopen(path, c"w".as_ptr());
        assert!(!f.is_null());

        let fd = stdio::fileno(f);
        assert!(fd >= 0, "fileno should return a valid fd");

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn popen_pclose() {
    unsafe {
        let cmd = c"echo hello".as_ptr();
        let mode = c"r".as_ptr();
        let f = stdio::popen(cmd, mode);
        assert!(!f.is_null(), "popen should return non-null");

        let mut buf = [0i8; 64];
        let result = stdio::fgets(buf.as_mut_ptr(), buf.len() as i32, f);
        assert!(!result.is_null());
        assert_eq!(&buf[..6], b"hello\n".map(|b| b as i8));

        let status = stdio::pclose(f);
        assert_eq!(status, 0, "pclose should return 0 for a successful command");
    }
}

#[test]
fn feof_after_read() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_eof".as_ptr();
        let f = stdio::fopen(path, c"w+".as_ptr());
        assert!(!f.is_null());

        // Write one byte
        stdio::fputc(b'X' as i32, f);
        stdio::rewind(f);

        // Read it
        stdio::fgetc(f);
        // Try to read past end
        stdio::fgetc(f);

        assert_ne!(
            stdio::feof(f),
            0,
            "feof should be non-zero after reading past end"
        );

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn ferror_on_write_to_readonly() {
    unsafe {
        let path = c"/tmp/bnd_stdio_test_ferror".as_ptr();
        // Create the file first
        let f = stdio::fopen(path, c"w".as_ptr());
        assert!(!f.is_null());
        stdio::fclose(f);

        // Open read-only
        let f = stdio::fopen(path, c"r".as_ptr());
        assert!(!f.is_null());

        // Write to a read-only stream
        stdio::fputc(b'X' as i32, f);
        assert_ne!(
            stdio::ferror(f),
            0,
            "ferror should be non-zero after writing to read-only stream"
        );

        stdio::fclose(f);
        stdio::remove(path);
    }
}

#[test]
fn tmpfile_creates_anonymous() {
    unsafe {
        let f = stdio::tmpfile();
        assert!(!f.is_null(), "tmpfile should return a non-null FILE*");

        let fd = stdio::fileno(f);
        assert!(fd >= 0, "tmpfile fd should be valid");

        let ret = stdio::fclose(f);
        assert_eq!(ret, 0);
    }
}

#[test]
fn fpos_t_layout() {
    // fpos_t wraps _G_fpos_t { __off_t (i64), __mbstate_t { i32, union(u32) } }
    // = 8 + 8 = 16 bytes
    assert_eq!(
        core::mem::size_of::<stdio::fpos_t>(),
        16,
        "fpos_t should be 16 bytes"
    );
}

#[test]
fn io_file_struct_size() {
    // _IO_FILE is 216 bytes on glibc x86-64
    assert_eq!(
        core::mem::size_of::<stdio::_IO_FILE>(),
        216,
        "_IO_FILE should be 216 bytes"
    );
}
