//! End-to-end tests for dlfcn bindings against real libc.

use bnd_posix::posix::dl;

#[test]
fn rtld_constants() {
    assert_eq!(dl::RTLD_LAZY, 0x1);
    assert_eq!(dl::RTLD_NOW, 0x2);
    assert_eq!(dl::RTLD_GLOBAL, 0x100);
    assert_eq!(dl::RTLD_LOCAL, 0);
    assert_eq!(dl::RTLD_NOLOAD, 0x4);
    assert_eq!(dl::RTLD_NODELETE, 0x1000);
    assert_eq!(dl::RTLD_DEEPBIND, 0x8);
}

#[test]
fn dlopen_libc() {
    unsafe {
        // Open libc — passing null opens the main program handle
        let handle = dl::dlopen(core::ptr::null(), dl::RTLD_LAZY);
        assert!(
            !handle.is_null(),
            "dlopen(NULL) should return a valid handle"
        );

        // Close the handle
        let ret = dl::dlclose(handle);
        assert_eq!(ret, 0, "dlclose should succeed");
    }
}

#[test]
fn dlsym_finds_getpid() {
    unsafe {
        let handle = dl::dlopen(core::ptr::null(), dl::RTLD_LAZY);
        assert!(!handle.is_null());

        let name = c"getpid";
        let sym = dl::dlsym(handle, name.as_ptr());
        assert!(
            !sym.is_null(),
            "dlsym should find 'getpid' in default handle"
        );

        dl::dlclose(handle);
    }
}

#[test]
fn dlsym_returns_null_for_missing() {
    unsafe {
        let handle = dl::dlopen(core::ptr::null(), dl::RTLD_LAZY);
        assert!(!handle.is_null());

        let name = c"__this_symbol_does_not_exist_12345__";
        let sym = dl::dlsym(handle, name.as_ptr());
        assert!(sym.is_null(), "dlsym should return null for missing symbol");

        // dlerror should return a non-null error message
        let err = dl::dlerror();
        assert!(
            !err.is_null(),
            "dlerror should return error after failed dlsym"
        );

        dl::dlclose(handle);
    }
}

#[test]
fn dlopen_nonexistent_returns_null() {
    unsafe {
        let name = c"/nonexistent/library.so";
        let handle = dl::dlopen(name.as_ptr(), dl::RTLD_LAZY);
        assert!(
            handle.is_null(),
            "dlopen should return null for nonexistent library"
        );

        // dlerror should return a non-null error message
        let err = dl::dlerror();
        assert!(
            !err.is_null(),
            "dlerror should return error after failed dlopen"
        );
    }
}

#[test]
fn dlerror_returns_null_when_no_error() {
    unsafe {
        // Clear any previous error
        let _ = dl::dlerror();

        // dlerror should return null when no error has occurred
        let err = dl::dlerror();
        assert!(
            err.is_null(),
            "dlerror should return null when no error pending"
        );
    }
}

#[test]
fn dlsym_returns_callable_function_pointer() {
    unsafe {
        let handle = dl::dlopen(core::ptr::null(), dl::RTLD_LAZY);
        assert!(!handle.is_null());

        let name = c"getpid";
        let sym = dl::dlsym(handle, name.as_ptr());
        assert!(!sym.is_null());

        // Cast to function pointer and call it
        let getpid: unsafe extern "C" fn() -> i32 = core::mem::transmute(sym);
        let pid = getpid();
        assert!(pid > 0, "getpid() via dlsym should return a positive PID");

        dl::dlclose(handle);
    }
}
