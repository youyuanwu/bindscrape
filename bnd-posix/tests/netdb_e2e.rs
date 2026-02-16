//! End-to-end tests for Netdb bindings against real libc.

use bnd_posix::posix::netdb;
use bnd_posix::posix::socket;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn ai_flag_constants() {
    assert_eq!(netdb::AI_PASSIVE, 1);
    assert_eq!(netdb::AI_CANONNAME, 2);
    assert_eq!(netdb::AI_NUMERICHOST, 4);
    assert_eq!(netdb::AI_V4MAPPED, 8);
    assert_eq!(netdb::AI_ADDRCONFIG, 32);
}

#[test]
fn eai_error_constants() {
    assert_eq!(netdb::EAI_NONAME, -2);
    assert_eq!(netdb::EAI_AGAIN, -3);
    assert_eq!(netdb::EAI_FAIL, -4);
    assert_eq!(netdb::EAI_FAMILY, -6);
    assert_eq!(netdb::EAI_MEMORY, -10);
}

// ---------------------------------------------------------------------------
// Struct layout
// ---------------------------------------------------------------------------

#[test]
fn addrinfo_struct_size() {
    assert_eq!(
        core::mem::size_of::<netdb::addrinfo>(),
        48,
        "struct addrinfo should be 48 bytes on x86_64"
    );
}

#[test]
fn hostent_struct_size() {
    assert_eq!(
        core::mem::size_of::<netdb::hostent>(),
        32,
        "struct hostent should be 32 bytes on x86_64"
    );
}

#[test]
fn servent_struct_size() {
    assert_eq!(
        core::mem::size_of::<netdb::servent>(),
        32,
        "struct servent should be 32 bytes on x86_64"
    );
}

#[test]
fn protoent_struct_size() {
    assert_eq!(
        core::mem::size_of::<netdb::protoent>(),
        24,
        "struct protoent should be 24 bytes on x86_64"
    );
}

#[test]
fn netent_struct_size() {
    assert_eq!(
        core::mem::size_of::<netdb::netent>(),
        24,
        "struct netent should be 24 bytes on x86_64"
    );
}

// ---------------------------------------------------------------------------
// Lookup functions
// ---------------------------------------------------------------------------

#[test]
fn getprotobyname_tcp() {
    let name = c"tcp";
    let entry = unsafe { netdb::getprotobyname(name.as_ptr()) };
    assert!(!entry.is_null(), "getprotobyname(\"tcp\") should succeed");
    let proto = unsafe { (*entry).p_proto };
    assert_eq!(proto, 6, "TCP protocol number should be 6");
}

#[test]
fn getprotobyname_udp() {
    let name = c"udp";
    let entry = unsafe { netdb::getprotobyname(name.as_ptr()) };
    assert!(!entry.is_null(), "getprotobyname(\"udp\") should succeed");
    let proto = unsafe { (*entry).p_proto };
    assert_eq!(proto, 17, "UDP protocol number should be 17");
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn getaddrinfo_localhost() {
    let node = c"127.0.0.1";
    let mut hints = netdb::addrinfo::default();
    hints.ai_family = socket::PF_INET;
    hints.ai_socktype = socket::SOCK_STREAM as i32;

    let mut result: *const netdb::addrinfo = core::ptr::null();
    let rc = unsafe {
        netdb::getaddrinfo(
            node.as_ptr(),
            core::ptr::null(),
            &hints as *const _,
            &mut result as *mut _ as *const *const netdb::addrinfo,
        )
    };
    assert_eq!(rc, 0, "getaddrinfo should succeed for 127.0.0.1");
    assert!(!result.is_null());

    let ai = unsafe { &*result };
    assert_eq!(ai.ai_family, socket::PF_INET);
    assert_eq!(ai.ai_socktype, socket::SOCK_STREAM as i32);

    unsafe { netdb::freeaddrinfo(result) };
}
