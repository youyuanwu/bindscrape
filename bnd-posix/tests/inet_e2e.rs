//! End-to-end tests for Inet bindings against real libc.

use bnd_posix::posix::inet;
use bnd_posix::posix::socket;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn ipproto_constants() {
    assert_eq!(inet::IPPROTO_TCP, 6);
    assert_eq!(inet::IPPROTO_UDP, 17);
    assert_eq!(inet::IPPROTO_ICMP, 1);
    assert_eq!(inet::IPPROTO_IP, 0);
    assert_eq!(inet::IPPROTO_IPV6, 41);
    assert_eq!(inet::IPPROTO_RAW, 255);
}

#[test]
fn inet_addrstrlen_constants() {
    assert_eq!(inet::INET_ADDRSTRLEN, 16);
    assert_eq!(inet::INET6_ADDRSTRLEN, 46);
}

// ---------------------------------------------------------------------------
// Struct layout
// ---------------------------------------------------------------------------

#[test]
fn in_addr_struct_size() {
    assert_eq!(
        core::mem::size_of::<inet::in_addr>(),
        4,
        "struct in_addr should be 4 bytes"
    );
}

#[test]
fn sockaddr_in_struct_size() {
    assert_eq!(
        core::mem::size_of::<inet::sockaddr_in>(),
        16,
        "struct sockaddr_in should be 16 bytes"
    );
}

#[test]
fn sockaddr_in6_struct_size() {
    assert_eq!(
        core::mem::size_of::<inet::sockaddr_in6>(),
        28,
        "struct sockaddr_in6 should be 28 bytes"
    );
}

#[test]
fn in6_addr_struct_size() {
    assert_eq!(
        core::mem::size_of::<inet::in6_addr>(),
        16,
        "struct in6_addr should be 16 bytes"
    );
}

// ---------------------------------------------------------------------------
// Byte order functions
// ---------------------------------------------------------------------------

#[test]
fn htons_ntohs_roundtrip() {
    let val: u16 = 0x1234;
    let net = unsafe { inet::htons(val) };
    let host = unsafe { inet::ntohs(net) };
    assert_eq!(host, val, "ntohs(htons(x)) should equal x");
}

#[test]
fn htonl_ntohl_roundtrip() {
    let val: u32 = 0xDEADBEEF;
    let net = unsafe { inet::htonl(val) };
    let host = unsafe { inet::ntohl(net) };
    assert_eq!(host, val, "ntohl(htonl(x)) should equal x");
}

// ---------------------------------------------------------------------------
// Address conversion
// ---------------------------------------------------------------------------

#[test]
fn inet_pton_ipv4() {
    let addr_str = c"127.0.0.1";
    let mut addr = inet::in_addr::default();
    let rc = unsafe {
        inet::inet_pton(
            socket::PF_INET,
            addr_str.as_ptr(),
            &mut addr as *mut _ as *const core::ffi::c_void,
        )
    };
    assert_eq!(rc, 1, "inet_pton should succeed");
    let expected = unsafe { inet::htonl(0x7f000001) };
    assert_eq!(addr.s_addr, expected, "parsed address should be 127.0.0.1");
}

#[test]
fn inet_addr_loopback() {
    let addr_str = c"127.0.0.1";
    let result = unsafe { inet::inet_addr(addr_str.as_ptr()) };
    let expected = unsafe { inet::htonl(0x7f000001) };
    assert_eq!(result, expected);
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn inet_ntop_ipv4() {
    let mut addr = inet::in_addr::default();
    addr.s_addr = unsafe { inet::htonl(0x0a000001) }; // 10.0.0.1
    let mut buf = [0i8; 16];
    let result = unsafe {
        inet::inet_ntop(
            socket::PF_INET,
            &addr as *const _ as *const core::ffi::c_void,
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    assert!(!result.is_null(), "inet_ntop should succeed");
    let s = unsafe { std::ffi::CStr::from_ptr(result) };
    assert_eq!(s.to_str().unwrap(), "10.0.0.1");
}
