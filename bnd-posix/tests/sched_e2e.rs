//! End-to-end tests for sched bindings against real libc.

use bnd_posix::posix::sched;

#[test]
fn sched_constants() {
    assert_eq!(sched::SCHED_OTHER, 0);
    assert_eq!(sched::SCHED_FIFO, 1);
    assert_eq!(sched::SCHED_RR, 2);
}

#[test]
fn sched_yield_succeeds() {
    unsafe {
        let ret = sched::sched_yield();
        assert_eq!(ret, 0, "sched_yield should succeed");
    }
}

#[test]
fn sched_get_priority_range() {
    unsafe {
        let min = sched::sched_get_priority_min(sched::SCHED_FIFO);
        let max = sched::sched_get_priority_max(sched::SCHED_FIFO);
        assert!(min >= 0, "priority min should be non-negative");
        assert!(max > min, "priority max should exceed min");
    }
}

#[test]
fn sched_getscheduler_self() {
    unsafe {
        let policy = sched::sched_getscheduler(0); // 0 = current process
        assert!(policy >= 0, "sched_getscheduler should succeed for self");
        // Default user process should be SCHED_OTHER
        assert_eq!(policy, sched::SCHED_OTHER);
    }
}

#[test]
fn cpu_set_t_size() {
    // cpu_set_t should hold __CPU_SETSIZE (1024) bits = 128 bytes
    assert_eq!(
        core::mem::size_of::<sched::cpu_set_t>(),
        128,
        "cpu_set_t should be 128 bytes"
    );
}

#[test]
fn sched_param_size() {
    assert_eq!(
        core::mem::size_of::<sched::sched_param>(),
        4,
        "sched_param should be 4 bytes (single i32)"
    );
}
