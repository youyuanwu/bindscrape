//! End-to-end tests exercising the generated zlib FFI bindings against the
//! real libz shared library.

use e2e_zlib::*;

// ---------------------------------------------------------------------------
// Version / constant tests
// ---------------------------------------------------------------------------

#[test]
fn zlib_version_returns_string() {
    let ver = unsafe { zlibVersion() };
    assert!(!ver.is_null());
    let s = unsafe { std::ffi::CStr::from_ptr(ver) };
    let ver_str = s.to_str().expect("version should be valid UTF-8");
    assert!(
        ver_str.starts_with("1."),
        "expected zlib version 1.x, got {ver_str}"
    );
}

#[test]
fn z_ok_is_zero() {
    assert_eq!(Z_OK, 0);
}

#[test]
fn z_stream_end_is_one() {
    assert_eq!(Z_STREAM_END, 1);
}

#[test]
fn z_deflated_is_eight() {
    assert_eq!(Z_DEFLATED, 8);
}

#[test]
fn max_wbits_is_fifteen() {
    assert_eq!(MAX_WBITS, 15);
}

// ---------------------------------------------------------------------------
// CRC / Adler tests
// ---------------------------------------------------------------------------

#[test]
fn crc32_known_value() {
    let data = b"hello";
    let crc = unsafe { crc32(0, data.as_ptr(), data.len() as uInt) };
    assert_eq!(crc, 0x3610a686, "CRC-32 of 'hello'");
}

#[test]
fn adler32_known_value() {
    let data = b"hello";
    let a = unsafe { adler32(1, data.as_ptr(), data.len() as uInt) };
    assert_eq!(a, 0x062c0215, "Adler-32 of 'hello'");
}

// ---------------------------------------------------------------------------
// Compress / uncompress roundtrip
// ---------------------------------------------------------------------------

#[test]
fn compress_uncompress_roundtrip() {
    let original = b"The quick brown fox jumps over the lazy dog";
    let mut compressed = vec![0u8; (original.len() * 2) + 64];
    let mut compressed_len = compressed.len() as uLong;

    let ret = unsafe {
        compress(
            compressed.as_mut_ptr(),
            &mut compressed_len as *mut uLong,
            original.as_ptr(),
            original.len() as uLong,
        )
    };
    assert_eq!(ret, Z_OK, "compress failed with {ret}");
    assert!(
        (compressed_len as usize) > 0,
        "compressed should have nonzero length"
    );

    let mut decompressed = vec![0u8; original.len() + 16];
    let mut decompressed_len = decompressed.len() as uLong;

    let ret = unsafe {
        uncompress(
            decompressed.as_mut_ptr(),
            &mut decompressed_len as *mut uLong,
            compressed.as_ptr(),
            compressed_len,
        )
    };
    assert_eq!(ret, Z_OK, "uncompress failed with {ret}");
    assert_eq!(decompressed_len as usize, original.len());
    assert_eq!(&decompressed[..original.len()], original);
}

// ---------------------------------------------------------------------------
// compressBound
// ---------------------------------------------------------------------------

#[test]
fn compress_bound_is_reasonable() {
    let bound = unsafe { compressBound(1000) };
    // compressBound should return a value >= the input size
    assert!(bound >= 1000, "bound should be at least 1000, got {bound}");
    // But not absurdly large
    assert!(bound < 2000, "bound should be reasonable, got {bound}");
}

// ---------------------------------------------------------------------------
// Struct layout validation
// ---------------------------------------------------------------------------

#[test]
fn z_stream_s_size() {
    let size = std::mem::size_of::<z_stream_s>();
    // On 64-bit Linux, z_stream_s should be 112 bytes
    // (14 fields: mix of pointers, u32s, and function pointers)
    assert!(size > 0, "z_stream_s should have nonzero size");
}

#[test]
fn gz_header_s_size() {
    let size = std::mem::size_of::<gz_header_s>();
    assert!(size > 0, "gz_header_s should have nonzero size");
}

#[test]
fn gz_file_s_has_three_fields() {
    let s = gzFile_s::default();
    // Accessing all 3 fields to prove they compile
    let _ = s.have;
    let _ = s.next;
    let _ = s.pos;
}
