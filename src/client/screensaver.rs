const WM_SYSCOMMAND: u32 = 0x0112;
const SC_SCREENSAVE: usize = 0xf140;

fn suppress(active: bool, message: u32, command: usize) -> bool {
    active && message == WM_SYSCOMMAND && command & 0xfff0 == SC_SCREENSAVE
}

#[cfg(windows)]
pub(super) struct Filter {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    _main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(windows)]
#[repr(C)]
struct MessagePrefix {
    window: *mut std::ffi::c_void,
    message: u32,
    command: usize,
}

#[cfg(windows)]
unsafe extern "C" fn filter(
    native: *mut gtk::gdk::ffi::GdkXEvent,
    _event: *mut gtk::gdk::ffi::GdkEvent,
    data: *mut std::ffi::c_void,
) -> gtk::gdk::ffi::GdkFilterReturn {
    use std::sync::atomic::{AtomicU8, Ordering};
    // SAFETY: Win32 GDK passes MSG; repr(C) preserves its HWND, UINT and WPARAM prefix.
    // Filter owns the Arc and unregisters this callback before releasing its allocation.
    if let (Some(message), Some(state)) = unsafe {
        (
            native.cast::<MessagePrefix>().as_ref(),
            data.cast::<AtomicU8>().as_ref(),
        )
    } && suppress(
        state.load(Ordering::Acquire) == super::playback::WATCHING,
        message.message,
        message.command,
    ) {
        gtk::gdk::ffi::GDK_FILTER_REMOVE
    } else {
        gtk::gdk::ffi::GDK_FILTER_CONTINUE
    }
}

#[cfg(windows)]
impl Filter {
    pub fn new(state: std::sync::Arc<std::sync::atomic::AtomicU8>) -> Self {
        assert!(gtk::is_initialized_main_thread());
        // SAFETY: global filter is installed/removed on GTK's thread; self retains its data.
        unsafe {
            gtk::gdk::ffi::gdk_window_add_filter(
                std::ptr::null_mut(),
                Some(filter),
                std::sync::Arc::as_ptr(&state).cast_mut().cast(),
            );
        }
        Self {
            state,
            _main_thread: std::marker::PhantomData,
        }
    }
}

#[cfg(windows)]
impl Drop for Filter {
    fn drop(&mut self) {
        // SAFETY: same callback and data as registration; !Send keeps cleanup on GTK's thread.
        unsafe {
            gtk::gdk::ffi::gdk_window_remove_filter(
                std::ptr::null_mut(),
                Some(filter),
                std::sync::Arc::as_ptr(&self.state).cast_mut().cast(),
            );
        }
    }
}

#[cfg(all(windows, target_pointer_width = "64"))]
const _: () = assert!(std::mem::offset_of!(MessagePrefix, command) == 16);
#[cfg(all(windows, target_pointer_width = "32"))]
const _: () = assert!(std::mem::offset_of!(MessagePrefix, command) == 8);

#[cfg(test)]
#[test]
fn only_blocks_the_screensaver_while_watching() {
    assert!(suppress(true, WM_SYSCOMMAND, SC_SCREENSAVE | 3));
    assert!(!suppress(false, WM_SYSCOMMAND, SC_SCREENSAVE));
    assert!(!suppress(true, WM_SYSCOMMAND, 0xf060)); // Close must still work.
    assert!(!suppress(true, 0x0100, SC_SCREENSAVE)); // Keyboard must still work.
}
