//! End-to-end tests for time bindings against real libc.
#![allow(clippy::unnecessary_mut_passed)]

use bnd_posix::posix::time;

#[test]
fn clock_constants() {
    assert_eq!(time::CLOCK_REALTIME, 0);
    assert_eq!(time::CLOCK_MONOTONIC, 1);
    assert_eq!(time::CLOCK_PROCESS_CPUTIME_ID, 2);
    assert_eq!(time::CLOCK_THREAD_CPUTIME_ID, 3);
    assert_eq!(time::CLOCK_MONOTONIC_RAW, 4);
    assert_eq!(time::CLOCK_REALTIME_COARSE, 5);
    assert_eq!(time::CLOCK_MONOTONIC_COARSE, 6);
    assert_eq!(time::CLOCK_BOOTTIME, 7);
    assert_eq!(time::CLOCK_TAI, 11);
    assert_eq!(time::TIMER_ABSTIME, 1);
}

#[test]
fn time_returns_epoch() {
    unsafe {
        let t = time::time(core::ptr::null());
        // Should return seconds since epoch — at least year 2024
        assert!(
            t > 1_700_000_000,
            "time() should return a recent epoch timestamp"
        );
    }
}

#[test]
fn clock_gettime_monotonic() {
    use bnd_posix::posix::stat; // timespec lives in stat partition
    unsafe {
        let mut ts: stat::timespec = core::mem::zeroed();
        let ret = time::clock_gettime(time::CLOCK_MONOTONIC, &mut ts);
        assert_eq!(ret, 0, "clock_gettime(CLOCK_MONOTONIC) should succeed");
        assert!(ts.tv_sec > 0, "monotonic clock should have elapsed seconds");
    }
}

#[test]
fn gmtime_epoch_zero() {
    unsafe {
        let epoch: i64 = 0;
        let tm = time::gmtime(&epoch);
        assert!(!tm.is_null(), "gmtime should return a valid pointer");
        let tm = &*tm;
        // Unix epoch: 1970-01-01 00:00:00 UTC
        assert_eq!(tm.tm_year, 70, "epoch year should be 70 (1900+70=1970)");
        assert_eq!(tm.tm_mon, 0, "epoch month should be 0 (January)");
        assert_eq!(tm.tm_mday, 1, "epoch day should be 1");
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_sec, 0);
    }
}

#[test]
fn mktime_roundtrip() {
    unsafe {
        // Start from a known epoch and roundtrip through gmtime + mktime
        let original: i64 = 1_000_000_000; // 2001-09-09
        let mut tm: time::tm = core::mem::zeroed();
        let result = time::gmtime_r(&original, &mut tm);
        assert!(!result.is_null());
        // mktime interprets as local time, but the roundtrip should be close
        let rebuilt = time::timegm(&mut tm);
        assert_eq!(rebuilt, original, "roundtrip should preserve the timestamp");
    }
}

#[test]
fn difftime_works() {
    unsafe {
        let diff = time::difftime(100, 50);
        assert!((diff - 50.0).abs() < f64::EPSILON);
    }
}

#[test]
fn struct_tm_layout() {
    // struct tm should have the standard POSIX layout
    let tm = time::tm::default();
    assert_eq!(tm.tm_sec, 0);
    assert_eq!(tm.tm_min, 0);
    assert_eq!(tm.tm_hour, 0);
    // Size should be reasonable (at least 44 bytes on x86_64 with gmtoff+zone)
    assert!(core::mem::size_of::<time::tm>() >= 44);
}

#[test]
fn tzset_runs() {
    unsafe {
        // tzset should not crash — it reads TZ env and sets internal state
        time::tzset();
    }
}
