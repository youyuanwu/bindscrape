mod bindings;

#[cfg(test)]
mod tests {
    use crate::bindings::MultiTest::Types::*;
    use crate::bindings::MultiTest::Widgets::*;

    // -----------------------------------------------------------------------
    // Namespace separation: types from types.h land in MultiTest::Types,
    // functions/Widget from widget.h land in MultiTest::Widgets.
    // Cross-partition references use super::Types:: paths (e.g.
    // Widget.color is super::Types::Color).
    // -----------------------------------------------------------------------

    #[test]
    fn test_constants() {
        assert_eq!(MAX_WIDGETS, 256);
        assert_eq!(DEFAULT_WIDTH, 800);
        assert_eq!(DEFAULT_HEIGHT, 600);
    }

    #[test]
    fn test_enum_values() {
        assert_eq!(COLOR_RED, 0u32);
        assert_eq!(COLOR_GREEN, 1u32);
        assert_eq!(COLOR_BLUE, 2u32);
    }

    #[test]
    fn test_struct_layout() {
        assert_eq!(std::mem::size_of::<Rect>(), 16);
        assert_eq!(std::mem::align_of::<Rect>(), 4);

        let r = Rect {
            x: 10,
            y: 20,
            width: 100,
            height: 200,
        };
        assert_eq!(r.x, 10);
        assert_eq!(r.width, 100);
    }

    #[test]
    fn test_widget_struct_has_color_field() {
        let w = Widget {
            name: std::ptr::null_mut(),
            values: [0; 4],
            color: COLOR_GREEN,
        };
        assert_eq!(w.color, 1u32);
    }

    #[test]
    fn test_cross_namespace_types() {
        // Widget.color type is super::Types::Color (resolved cross-partition)
        use crate::bindings::MultiTest::Types::Color;
        let _color: Color = COLOR_BLUE;

        // Widget is in Widgets, Color is in Types — they compose correctly
        let w = Widget {
            name: std::ptr::null_mut(),
            values: [0; 4],
            color: COLOR_RED,
        };
        assert_eq!(w.color, 0u32);
    }

    #[test]
    fn test_create_and_destroy_widget() {
        unsafe {
            assert_eq!(widget_count(), 0);

            let name = c"multi".as_ptr();
            let bounds = Rect {
                x: 0,
                y: 0,
                width: 640,
                height: 480,
            };
            let mut widget: Widget = std::mem::zeroed();

            let result = create_widget(name, bounds, &mut widget as *mut Widget as *const Widget);
            assert_eq!(result, 0);
            assert_eq!(widget_count(), 1);
            assert_eq!(widget.color, COLOR_RED);
            assert_eq!(widget.values[2], 640);

            let returned_name = std::ffi::CStr::from_ptr(widget.name);
            assert_eq!(returned_name.to_str().unwrap(), "multi");

            destroy_widget(&mut widget as *mut Widget as *const Widget);
            assert_eq!(widget_count(), 0);
        }
    }

    #[test]
    fn test_create_widget_null_returns_error() {
        unsafe {
            let bounds = Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
            let result = create_widget(std::ptr::null(), bounds, std::ptr::null());
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_delegate_type_exists() {
        unsafe extern "system" fn cmp(
            a: *const core::ffi::c_void,
            b: *const core::ffi::c_void,
        ) -> i32 {
            unsafe {
                let a = *(a as *const i32);
                let b = *(b as *const i32);
                a - b
            }
        }
        let _f: CompareFunc = Some(cmp);
    }
}
