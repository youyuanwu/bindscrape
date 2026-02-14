//! Rust implementation of the simple.h C API.
//!
//! Compiled as a cdylib so it can be linked by the e2e test crates
//! through the generated P/Invoke bindings.

use std::sync::atomic::{AtomicI32, Ordering};

// Mirror the C types from simple.h
#[repr(C)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[repr(i32)]
pub enum Color {
    Red = 0,
    Green = 1,
    Blue = 2,
}

#[repr(C)]
pub struct Widget {
    pub name: *const i8,
    pub values: [i32; 4],
    pub color: i32, // Color enum as i32
}

static WIDGET_COUNT: AtomicI32 = AtomicI32::new(0);

#[unsafe(no_mangle)]
/// # Safety
/// `name` and `out` must be valid, non-null pointers.
pub unsafe extern "C" fn create_widget(name: *const i8, bounds: Rect, out: *mut Widget) -> i32 {
    if out.is_null() || name.is_null() {
        return -1;
    }
    unsafe {
        (*out).name = name;
        (*out).values[0] = bounds.x;
        (*out).values[1] = bounds.y;
        (*out).values[2] = bounds.width as i32;
        (*out).values[3] = bounds.height as i32;
        (*out).color = Color::Red as i32;
    }
    WIDGET_COUNT.fetch_add(1, Ordering::Relaxed);
    0
}

#[unsafe(no_mangle)]
/// # Safety
/// `w` must be a valid, non-null pointer to a `Widget`.
pub unsafe extern "C" fn destroy_widget(w: *mut Widget) {
    if w.is_null() {
        return;
    }
    unsafe {
        (*w).name = std::ptr::null();
    }
    WIDGET_COUNT.fetch_sub(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn widget_count() -> i32 {
    WIDGET_COUNT.load(Ordering::Relaxed)
}
