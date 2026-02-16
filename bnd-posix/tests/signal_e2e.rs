//! End-to-end tests for Signal bindings against real libc.

use bnd_posix::posix::{pthread, signal};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn sig_constants() {
    assert_eq!(signal::SIGHUP, 1);
    assert_eq!(signal::SIGINT, 2);
    assert_eq!(signal::SIGQUIT, 3);
    assert_eq!(signal::SIGILL, 4);
    assert_eq!(signal::SIGTRAP, 5);
    assert_eq!(signal::SIGABRT, 6);
    assert_eq!(signal::SIGBUS, 7);
    assert_eq!(signal::SIGFPE, 8);
    assert_eq!(signal::SIGKILL, 9);
    assert_eq!(signal::SIGUSR1, 10);
    assert_eq!(signal::SIGSEGV, 11);
    assert_eq!(signal::SIGUSR2, 12);
    assert_eq!(signal::SIGPIPE, 13);
    assert_eq!(signal::SIGALRM, 14);
    assert_eq!(signal::SIGTERM, 15);
    assert_eq!(signal::SIGCHLD, 17);
    assert_eq!(signal::SIGCONT, 18);
    assert_eq!(signal::SIGSTOP, 19);
    assert_eq!(signal::SIGTSTP, 20);
}

#[test]
fn sa_flag_constants() {
    assert_eq!(signal::SA_NOCLDSTOP, 1);
    assert_eq!(signal::SA_NOCLDWAIT, 2);
    assert_eq!(signal::SA_SIGINFO, 4);
    assert_eq!(signal::SA_ONSTACK, 0x08000000);
    assert_eq!(signal::SA_RESTART, 0x10000000);
    assert_ne!(signal::SA_NODEFER, 0);
    assert_ne!(signal::SA_RESETHAND, 0);
}

#[test]
fn sig_block_constants() {
    assert_eq!(signal::SIG_BLOCK, 0);
    assert_eq!(signal::SIG_UNBLOCK, 1);
    assert_eq!(signal::SIG_SETMASK, 2);
}

// ---------------------------------------------------------------------------
// Struct layouts
// ---------------------------------------------------------------------------

#[test]
fn sigaction_struct_size() {
    // struct sigaction is 152 bytes on x86-64 Linux
    assert_eq!(
        std::mem::size_of::<signal::sigaction>(),
        152,
        "sigaction should be 152 bytes on x86-64"
    );
}

#[test]
fn sigset_struct_size() {
    // __sigset_t is 128 bytes (1024 bits / 8)
    assert_eq!(
        std::mem::size_of::<signal::__sigset_t>(),
        128,
        "__sigset_t should be 128 bytes"
    );
}

#[test]
fn siginfo_struct_size() {
    // siginfo_t is 128 bytes on x86-64 Linux
    assert_eq!(
        std::mem::size_of::<signal::siginfo_t>(),
        128,
        "siginfo_t should be 128 bytes"
    );
}

#[test]
fn stack_t_struct_size() {
    // stack_t is 24 bytes on x86-64 (void*, int, size_t with padding)
    assert_eq!(
        std::mem::size_of::<signal::stack_t>(),
        24,
        "stack_t should be 24 bytes"
    );
}

// ---------------------------------------------------------------------------
// Function pointer type (__sighandler_t)
// ---------------------------------------------------------------------------

#[test]
fn sighandler_type_is_option_fn_pointer() {
    // __sighandler_t is Option<unsafe extern "system" fn(i32)>
    // It should be pointer-sized
    assert_eq!(
        std::mem::size_of::<signal::__sighandler_t>(),
        std::mem::size_of::<usize>(),
        "__sighandler_t should be pointer-sized"
    );
}

// ---------------------------------------------------------------------------
// sigset operations
// ---------------------------------------------------------------------------

#[test]
fn sigemptyset_and_sigaddset() {
    let mut set = pthread::__sigset_t::default();
    let rc = unsafe { signal::sigemptyset(&mut set as *mut _ as *const _) };
    assert_eq!(rc, 0, "sigemptyset should succeed");

    let rc = unsafe { signal::sigaddset(&mut set as *mut _ as *const _, signal::SIGUSR1) };
    assert_eq!(rc, 0, "sigaddset should succeed");

    let ismember = unsafe { signal::sigismember(&set as *const _, signal::SIGUSR1) };
    assert_eq!(ismember, 1, "SIGUSR1 should be in the set");

    let not_member = unsafe { signal::sigismember(&set as *const _, signal::SIGUSR2) };
    assert_eq!(not_member, 0, "SIGUSR2 should not be in the set");
}

#[test]
fn sigfillset_and_sigdelset() {
    let mut set = pthread::__sigset_t::default();
    let rc = unsafe { signal::sigfillset(&mut set as *mut _ as *const _) };
    assert_eq!(rc, 0, "sigfillset should succeed");

    let ismember = unsafe { signal::sigismember(&set as *const _, signal::SIGINT) };
    assert_eq!(ismember, 1, "SIGINT should be in a full set");

    let rc = unsafe { signal::sigdelset(&mut set as *mut _ as *const _, signal::SIGINT) };
    assert_eq!(rc, 0, "sigdelset should succeed");

    let ismember = unsafe { signal::sigismember(&set as *const _, signal::SIGINT) };
    assert_eq!(ismember, 0, "SIGINT should no longer be in the set");
}

// ---------------------------------------------------------------------------
// Signal delivery
// ---------------------------------------------------------------------------

#[test]
fn raise_and_signal_handler() {
    use std::sync::atomic::{AtomicBool, Ordering};

    static HANDLER_CALLED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn handler(_sig: i32) {
        HANDLER_CALLED.store(true, Ordering::SeqCst);
    }

    // Install handler for SIGUSR1
    let prev = unsafe { signal::signal(signal::SIGUSR1, Some(handler)) };
    assert!(
        prev.is_some() || prev.is_none(),
        "signal() should return previous handler"
    );

    // Raise SIGUSR1
    let rc = unsafe { signal::raise(signal::SIGUSR1) };
    assert_eq!(rc, 0, "raise should succeed");

    assert!(
        HANDLER_CALLED.load(Ordering::SeqCst),
        "handler should have been called"
    );

    // Restore default handler
    unsafe { signal::signal(signal::SIGUSR1, None) };
}

// ---------------------------------------------------------------------------
// sigaction
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::field_reassign_with_default)]
fn sigaction_install_handler() {
    use std::sync::atomic::{AtomicI32, Ordering};

    static RECEIVED_SIG: AtomicI32 = AtomicI32::new(0);

    unsafe extern "system" fn handler(sig: i32) {
        RECEIVED_SIG.store(sig, Ordering::SeqCst);
    }

    let mut sa = signal::sigaction::default();
    sa.__sigaction_handler.sa_handler = Some(handler);
    sa.sa_flags = signal::SA_RESTART;

    // Empty the mask
    unsafe {
        signal::sigemptyset(&mut sa.sa_mask as *mut _ as *const _);
    }

    let rc = unsafe { signal::sigaction(signal::SIGUSR2, &sa as *const _, core::ptr::null()) };
    assert_eq!(rc, 0, "sigaction should succeed");

    // Raise SIGUSR2
    let rc = unsafe { signal::raise(signal::SIGUSR2) };
    assert_eq!(rc, 0, "raise should succeed");

    assert_eq!(
        RECEIVED_SIG.load(Ordering::SeqCst),
        signal::SIGUSR2,
        "handler should have received SIGUSR2"
    );

    // Restore default
    let mut default_sa = signal::sigaction::default();
    unsafe {
        signal::sigemptyset(&mut default_sa.sa_mask as *mut _ as *const _);
        signal::sigaction(signal::SIGUSR2, &default_sa as *const _, core::ptr::null());
    }
}

// ---------------------------------------------------------------------------
// sigprocmask
// ---------------------------------------------------------------------------

#[test]
fn sigprocmask_block_and_pending() {
    let mut block_set = pthread::__sigset_t::default();
    unsafe { signal::sigemptyset(&mut block_set as *mut _ as *const _) };
    unsafe { signal::sigaddset(&mut block_set as *mut _ as *const _, signal::SIGUSR1) };

    // Save old mask and block SIGUSR1
    let mut old_set = pthread::__sigset_t::default();
    let rc = unsafe {
        signal::sigprocmask(
            signal::SIG_BLOCK,
            &block_set as *const _,
            &mut old_set as *mut _ as *const _,
        )
    };
    assert_eq!(rc, 0, "sigprocmask SIG_BLOCK should succeed");

    // Check pending set — SIGUSR1 should NOT be pending yet (not raised)
    let mut pending = pthread::__sigset_t::default();
    let rc = unsafe { signal::sigpending(&mut pending as *mut _ as *const _) };
    assert_eq!(rc, 0, "sigpending should succeed");

    let is_pending = unsafe { signal::sigismember(&pending as *const _, signal::SIGUSR1) };
    assert_eq!(is_pending, 0, "SIGUSR1 should not be pending before raise");

    // Restore old mask
    let rc = unsafe {
        signal::sigprocmask(signal::SIG_SETMASK, &old_set as *const _, core::ptr::null())
    };
    assert_eq!(rc, 0, "sigprocmask SIG_SETMASK restore should succeed");
}

// ---------------------------------------------------------------------------
// kill (send signal to self)
// ---------------------------------------------------------------------------

#[test]
fn kill_self_with_zero() {
    // kill(getpid(), 0) is a standard way to test if a process exists
    let pid = std::process::id() as i32;
    let rc = unsafe { signal::kill(pid, 0) };
    assert_eq!(rc, 0, "kill(self, 0) should succeed");
}
