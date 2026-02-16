//! End-to-end tests for pthread bindings against real libc.
#![allow(clippy::unnecessary_mut_passed)]

use bnd_posix::posix::pthread;

#[test]
fn pthread_constants() {
    assert_eq!(pthread::PTHREAD_CREATE_JOINABLE, 0);
    assert_eq!(pthread::PTHREAD_CREATE_DETACHED, 1);
    assert_eq!(pthread::PTHREAD_MUTEX_NORMAL, 0);
    assert_eq!(pthread::PTHREAD_MUTEX_RECURSIVE, 1);
    assert_eq!(pthread::PTHREAD_MUTEX_ERRORCHECK, 2);
    assert_eq!(pthread::PTHREAD_MUTEX_DEFAULT, 0);
    assert_eq!(pthread::PTHREAD_CANCEL_ENABLE, 0);
    assert_eq!(pthread::PTHREAD_CANCEL_DISABLE, 1);
    assert_eq!(pthread::PTHREAD_CANCEL_DEFERRED, 0);
    assert_eq!(pthread::PTHREAD_CANCEL_ASYNCHRONOUS, 1);
    assert_eq!(pthread::PTHREAD_ONCE_INIT, 0);
    assert_eq!(pthread::PTHREAD_BARRIER_SERIAL_THREAD, -1);
    assert_eq!(pthread::PTHREAD_SCOPE_SYSTEM, 0);
    assert_eq!(pthread::PTHREAD_SCOPE_PROCESS, 1);
}

#[test]
fn pthread_self_returns_nonzero() {
    unsafe {
        let tid = pthread::pthread_self();
        assert!(tid != 0, "pthread_self should return a non-zero thread id");
    }
}

#[test]
fn pthread_equal_self() {
    unsafe {
        let tid = pthread::pthread_self();
        let equal = pthread::pthread_equal(tid, tid);
        assert!(equal != 0, "a thread should be equal to itself");
    }
}

#[test]
fn mutex_init_lock_unlock_destroy() {
    unsafe {
        let mut mutex: pthread::pthread_mutex_t = core::mem::zeroed();
        let ret = pthread::pthread_mutex_init(&mut mutex, core::ptr::null());
        assert_eq!(ret, 0, "pthread_mutex_init should succeed");

        let ret = pthread::pthread_mutex_lock(&mut mutex);
        assert_eq!(ret, 0, "pthread_mutex_lock should succeed");

        let ret = pthread::pthread_mutex_unlock(&mut mutex);
        assert_eq!(ret, 0, "pthread_mutex_unlock should succeed");

        let ret = pthread::pthread_mutex_destroy(&mut mutex);
        assert_eq!(ret, 0, "pthread_mutex_destroy should succeed");
    }
}

#[test]
fn mutex_trylock() {
    unsafe {
        let mut mutex: pthread::pthread_mutex_t = core::mem::zeroed();
        pthread::pthread_mutex_init(&mut mutex, core::ptr::null());

        let ret = pthread::pthread_mutex_trylock(&mut mutex);
        assert_eq!(ret, 0, "trylock on unlocked mutex should succeed");

        // Second trylock should fail with EBUSY (16)
        let ret = pthread::pthread_mutex_trylock(&mut mutex);
        assert_eq!(ret, 16, "trylock on locked mutex should return EBUSY");

        pthread::pthread_mutex_unlock(&mut mutex);
        pthread::pthread_mutex_destroy(&mut mutex);
    }
}

#[test]
fn rwlock_read_write() {
    unsafe {
        let mut rwlock: pthread::pthread_rwlock_t = core::mem::zeroed();
        let ret = pthread::pthread_rwlock_init(&mut rwlock, core::ptr::null());
        assert_eq!(ret, 0);

        // Read lock
        let ret = pthread::pthread_rwlock_rdlock(&mut rwlock);
        assert_eq!(ret, 0, "rdlock should succeed");
        let ret = pthread::pthread_rwlock_unlock(&mut rwlock);
        assert_eq!(ret, 0);

        // Write lock
        let ret = pthread::pthread_rwlock_wrlock(&mut rwlock);
        assert_eq!(ret, 0, "wrlock should succeed");
        let ret = pthread::pthread_rwlock_unlock(&mut rwlock);
        assert_eq!(ret, 0);

        pthread::pthread_rwlock_destroy(&mut rwlock);
    }
}

#[test]
fn pthread_key_create_delete() {
    unsafe {
        let mut key: pthread::pthread_key_t = 0;
        let ret = pthread::pthread_key_create(&mut key, core::ptr::null());
        assert_eq!(ret, 0, "pthread_key_create should succeed");

        // Set and get thread-specific data
        let val = 42usize as *const core::ffi::c_void;
        let ret = pthread::pthread_setspecific(key, val);
        assert_eq!(ret, 0, "pthread_setspecific should succeed");

        let got = pthread::pthread_getspecific(key);
        assert_eq!(
            got as usize, 42,
            "pthread_getspecific should return the stored value"
        );

        let ret = pthread::pthread_key_delete(key);
        assert_eq!(ret, 0, "pthread_key_delete should succeed");
    }
}

#[test]
fn pthread_create_join() {
    // pthread_create's start_routine is emitted as *const isize (opaque function pointer).
    // We transmute a Rust extern "C" fn into that type.
    unsafe extern "C" fn thread_fn(arg: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
        // Double the input value
        let val = arg as usize;
        (val * 2) as *mut core::ffi::c_void
    }

    unsafe {
        let mut tid: pthread::pthread_t = 0;
        let arg = 21usize as *mut core::ffi::c_void;

        // Cast function pointer to *const isize (the WinMD/bnd-winmd representation)
        let start_routine: *const isize = thread_fn as *const isize;

        let ret = pthread::pthread_create(&mut tid, core::ptr::null(), start_routine, arg);
        assert_eq!(ret, 0, "pthread_create should succeed");

        let mut result: *mut core::ffi::c_void = core::ptr::null_mut();
        let ret = pthread::pthread_join(
            tid,
            &mut result as *mut *mut core::ffi::c_void as *const *const core::ffi::c_void,
        );
        assert_eq!(ret, 0, "pthread_join should succeed");
        assert_eq!(result as usize, 42, "thread should return arg * 2");
    }
}

#[test]
fn pthread_attr_init_destroy() {
    unsafe {
        let mut attr: pthread::pthread_attr_t = core::mem::zeroed();
        let ret = pthread::pthread_attr_init(&mut attr);
        assert_eq!(ret, 0, "pthread_attr_init should succeed");

        let mut detach_state: i32 = -1;
        let ret = pthread::pthread_attr_getdetachstate(&attr, &mut detach_state);
        assert_eq!(ret, 0);
        assert_eq!(
            detach_state,
            pthread::PTHREAD_CREATE_JOINABLE as i32,
            "default detach state should be JOINABLE"
        );

        let ret = pthread::pthread_attr_destroy(&mut attr);
        assert_eq!(ret, 0, "pthread_attr_destroy should succeed");
    }
}

#[test]
fn spinlock_lock_unlock() {
    unsafe {
        let mut lock: pthread::pthread_spinlock_t = 0;
        let ret = pthread::pthread_spin_init(&mut lock, 0); // PTHREAD_PROCESS_PRIVATE
        assert_eq!(ret, 0);

        let ret = pthread::pthread_spin_lock(&mut lock);
        assert_eq!(ret, 0);

        let ret = pthread::pthread_spin_unlock(&mut lock);
        assert_eq!(ret, 0);

        pthread::pthread_spin_destroy(&mut lock);
    }
}

#[test]
fn struct_sizes() {
    // Verify key struct sizes match x86_64 glibc expectations
    assert_eq!(core::mem::size_of::<pthread::pthread_mutex_t>(), 40);
    assert_eq!(core::mem::size_of::<pthread::pthread_cond_t>(), 48);
    assert_eq!(core::mem::size_of::<pthread::pthread_rwlock_t>(), 56);
    assert_eq!(core::mem::size_of::<pthread::pthread_attr_t>(), 56);
    assert_eq!(core::mem::size_of::<pthread::pthread_barrier_t>(), 32);
}
