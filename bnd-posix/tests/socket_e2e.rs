//! End-to-end tests for Socket bindings against real libc.

use bnd_posix::posix::inet;
use bnd_posix::posix::socket;
use bnd_posix::posix::unistd;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn sock_type_constants() {
    assert_eq!(socket::SOCK_STREAM, 1);
    assert_eq!(socket::SOCK_DGRAM, 2);
    assert_eq!(socket::SOCK_RAW, 3);
    assert_eq!(socket::SOCK_SEQPACKET, 5);
}

#[test]
fn pf_inet_constants() {
    assert_eq!(socket::PF_INET, 2);
    assert_eq!(socket::PF_INET6, 10);
    assert_eq!(socket::PF_LOCAL, 1);
    assert_eq!(socket::PF_UNSPEC, 0);
}

#[test]
fn shutdown_constants() {
    assert_eq!(socket::SHUT_RD, 0);
    assert_eq!(socket::SHUT_WR, 1);
    assert_eq!(socket::SHUT_RDWR, 2);
}

#[test]
fn msg_flag_constants() {
    assert_eq!(socket::MSG_OOB, 1);
    assert_eq!(socket::MSG_PEEK, 2);
    assert_eq!(socket::MSG_DONTROUTE, 4);
    assert_eq!(socket::MSG_DONTWAIT, 64);
    assert_eq!(socket::MSG_WAITALL, 256);
}

// ---------------------------------------------------------------------------
// Struct layout
// ---------------------------------------------------------------------------

#[test]
fn sockaddr_struct_size() {
    assert_eq!(
        core::mem::size_of::<socket::sockaddr>(),
        16,
        "struct sockaddr should be 16 bytes"
    );
}

#[test]
fn sockaddr_storage_struct_size() {
    assert_eq!(
        core::mem::size_of::<socket::sockaddr_storage>(),
        128,
        "struct sockaddr_storage should be 128 bytes"
    );
}

#[test]
fn msghdr_struct_size() {
    assert_eq!(
        core::mem::size_of::<socket::msghdr>(),
        56,
        "struct msghdr should be 56 bytes on x86_64"
    );
}

#[test]
fn iovec_struct_size() {
    assert_eq!(
        core::mem::size_of::<socket::iovec>(),
        16,
        "struct iovec should be 16 bytes"
    );
}

#[test]
fn linger_struct_size() {
    assert_eq!(
        core::mem::size_of::<socket::linger>(),
        8,
        "struct linger should be 8 bytes"
    );
}

// ---------------------------------------------------------------------------
// Syscall smoke tests
// ---------------------------------------------------------------------------

#[test]
fn socket_create_tcp() {
    let fd = unsafe { socket::socket(socket::PF_INET, socket::SOCK_STREAM as i32, 0) };
    assert!(fd >= 0, "socket(PF_INET, SOCK_STREAM, 0) failed: {fd}");
    unsafe { unistd::close(fd) };
}

#[test]
fn socket_create_udp() {
    let fd = unsafe { socket::socket(socket::PF_INET, socket::SOCK_DGRAM as i32, 0) };
    assert!(fd >= 0, "socket(PF_INET, SOCK_DGRAM, 0) failed: {fd}");
    unsafe { unistd::close(fd) };
}

#[test]
fn socketpair_unix() {
    let mut fds = [0i32; 2];
    let rc = unsafe {
        socket::socketpair(
            socket::PF_LOCAL,
            socket::SOCK_STREAM as i32,
            0,
            fds.as_mut_ptr() as *const i32,
        )
    };
    assert_eq!(rc, 0, "socketpair failed");
    assert!(fds[0] >= 0);
    assert!(fds[1] >= 0);
    unsafe {
        unistd::close(fds[0]);
        unistd::close(fds[1]);
    };
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn getsockname_after_bind() {
    let fd = unsafe { socket::socket(socket::PF_INET, socket::SOCK_STREAM as i32, 0) };
    assert!(fd >= 0);

    let mut addr = inet::sockaddr_in::default();
    addr.sin_family = socket::PF_INET as u16;
    addr.sin_port = 0;
    addr.sin_addr.s_addr = unsafe { inet::htonl(0x7f000001) }; // 127.0.0.1

    let rc = unsafe {
        socket::bind(
            fd,
            &addr as *const _ as *const socket::sockaddr,
            core::mem::size_of::<inet::sockaddr_in>() as u32,
        )
    };
    assert_eq!(rc, 0, "bind to loopback:0 failed");

    let mut out = inet::sockaddr_in::default();
    let mut len = core::mem::size_of::<inet::sockaddr_in>() as u32;
    let rc = unsafe {
        socket::getsockname(
            fd,
            &mut out as *mut _ as *const socket::sockaddr,
            &mut len as *mut u32 as *const u32,
        )
    };
    assert_eq!(rc, 0, "getsockname failed");
    assert_eq!(out.sin_family, socket::PF_INET as u16);
    assert_ne!(out.sin_port, 0, "kernel should assign a port");

    unsafe { unistd::close(fd) };
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn listen_on_tcp_socket() {
    let fd = unsafe { socket::socket(socket::PF_INET, socket::SOCK_STREAM as i32, 0) };
    assert!(fd >= 0);

    let mut addr = inet::sockaddr_in::default();
    addr.sin_family = socket::PF_INET as u16;
    addr.sin_port = 0;
    addr.sin_addr.s_addr = unsafe { inet::htonl(0x7f000001) };

    let rc = unsafe {
        socket::bind(
            fd,
            &addr as *const _ as *const socket::sockaddr,
            core::mem::size_of::<inet::sockaddr_in>() as u32,
        )
    };
    assert_eq!(rc, 0, "bind failed");

    let rc = unsafe { socket::listen(fd, 5) };
    assert_eq!(rc, 0, "listen failed");

    unsafe { unistd::close(fd) };
}

#[test]
fn setsockopt_reuseaddr() {
    let fd = unsafe { socket::socket(socket::PF_INET, socket::SOCK_STREAM as i32, 0) };
    assert!(fd >= 0);

    let optval: i32 = 1;
    let rc = unsafe {
        socket::setsockopt(
            fd,
            1, // SOL_SOCKET
            2, // SO_REUSEADDR
            &optval as *const _ as *const core::ffi::c_void,
            core::mem::size_of::<i32>() as u32,
        )
    };
    assert_eq!(rc, 0, "setsockopt SO_REUSEADDR failed");

    unsafe { unistd::close(fd) };
}

#[test]
fn send_recv_socketpair() {
    let mut fds = [0i32; 2];
    let rc = unsafe {
        socket::socketpair(
            socket::PF_LOCAL,
            socket::SOCK_STREAM as i32,
            0,
            fds.as_mut_ptr() as *const i32,
        )
    };
    assert_eq!(rc, 0);

    let msg = b"hello socket";
    let sent = unsafe {
        socket::send(
            fds[0],
            msg.as_ptr() as *const core::ffi::c_void,
            msg.len() as u64,
            0,
        )
    };
    assert_eq!(sent, msg.len() as i64);

    let mut buf = [0u8; 64];
    let recvd = unsafe {
        socket::recv(
            fds[1],
            buf.as_mut_ptr() as *const core::ffi::c_void,
            buf.len() as u64,
            0,
        )
    };
    assert_eq!(recvd, msg.len() as i64);
    assert_eq!(&buf[..msg.len()], msg);

    unsafe {
        unistd::close(fds[0]);
        unistd::close(fds[1]);
    };
}
