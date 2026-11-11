#[cfg(not(windows))]
pub fn list() -> Vec<(u64, String)> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return vec![(0, "Escolher tela ou janela no portal".into())];
    }
    x11_windows()
}
#[cfg(target_os = "linux")]
fn x11_windows() -> Vec<(u64, String)> {
    use std::{
        ffi::{CStr, c_void},
        os::raw::{c_char, c_int, c_long, c_ulong},
    };
    #[link(name = "X11")]
    unsafe extern "C" {
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XInternAtom(display: *mut c_void, name: *const c_char, only_existing: c_int) -> c_ulong;
        fn XGetWindowProperty(
            display: *mut c_void,
            window: c_ulong,
            property: c_ulong,
            offset: c_long,
            length: c_long,
            delete: c_int,
            req_type: c_ulong,
            actual_type: *mut c_ulong,
            format: *mut c_int,
            count: *mut c_ulong,
            after: *mut c_ulong,
            data: *mut *mut u8,
        ) -> c_int;
        fn XFetchName(display: *mut c_void, window: c_ulong, name: *mut *mut c_char) -> c_int;
        fn XFree(data: *mut c_void) -> c_int;
    }
    #[link(name = "gdk-3")]
    unsafe extern "C" {
        fn gdk_x11_display_get_xdisplay(display: *mut c_void) -> *mut c_void;
        fn gdk_x11_display_error_trap_push(display: *mut c_void);
        fn gdk_x11_display_error_trap_pop_ignored(display: *mut c_void);
    }
    use gtk::glib::object::ObjectType;
    let mut windows = vec![(0, "Tela inteira (X11)".into())];
    let Some(gdk_display) = gtk::gdk::Display::default() else {
        return windows;
    };
    unsafe {
        let gdk = gdk_display.as_ptr().cast();
        let display = gdk_x11_display_get_xdisplay(gdk);
        if display.is_null() {
            return windows;
        }
        // A window may disappear between enumeration and reading its name.
        gdk_x11_display_error_trap_push(gdk);
        let property = XInternAtom(display, c"_NET_CLIENT_LIST".as_ptr(), 1);
        if property != 0 {
            let (mut actual, mut format, mut count, mut after) = (0, 0, 0, 0);
            let mut data = std::ptr::null_mut();
            let result = XGetWindowProperty(
                display,
                XDefaultRootWindow(display),
                property,
                0,
                4096,
                0,
                0,
                &mut actual,
                &mut format,
                &mut count,
                &mut after,
                &mut data,
            );
            if result == 0 && format == 32 && !data.is_null() && count <= 4096 {
                for &id in std::slice::from_raw_parts(data.cast::<c_ulong>(), count as usize) {
                    let mut name = std::ptr::null_mut();
                    if XFetchName(display, id, &mut name) != 0 && !name.is_null() {
                        let title = CStr::from_ptr(name).to_string_lossy().into_owned();
                        if !title.is_empty() && title != "Janja" {
                            windows.push((id, title));
                        }
                        XFree(name.cast());
                    }
                }
            }
            if !data.is_null() {
                XFree(data.cast());
            }
        }
        gdk_x11_display_error_trap_pop_ignored(gdk);
    }
    windows
}
#[cfg(windows)]
pub fn list() -> Vec<(u64, String)> {
    use std::ffi::c_void;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(
            callback: unsafe extern "system" fn(*mut c_void, isize) -> i32,
            parameter: isize,
        ) -> i32;
        fn IsWindowVisible(window: *mut c_void) -> i32;
        fn IsIconic(window: *mut c_void) -> i32;
        fn GetWindowTextW(window: *mut c_void, text: *mut u16, max: i32) -> i32;
        fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
    }
    unsafe extern "system" fn add(window: *mut c_void, data: isize) -> i32 {
        unsafe {
            if IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
                return 1;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(window, &mut pid);
            if pid == std::process::id() {
                return 1;
            }
            let mut title = [0u16; 512];
            let length = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32);
            if length > 0 {
                let items = &mut *(data as *mut Vec<(u64, String)>);
                items.push((
                    window as usize as u64,
                    String::from_utf16_lossy(&title[..length as usize]),
                ));
            }
            1
        }
    }
    let mut items = vec![(0, "Tela inteira".into())];
    unsafe {
        EnumWindows(add, &mut items as *mut _ as isize);
    }
    items
}
