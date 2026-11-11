use gtk::{glib::object::ObjectType, prelude::*};
use std::{cell::RefCell, ffi::c_void, rc::Rc};

#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
#[derive(Default)]
struct Size {
    width: i32,
    height: i32,
}
#[repr(C)]
struct Properties {
    flags: u32,
    destination: Rect,
    source: Rect,
    opacity: u8,
    visible: i32,
    client_only: i32,
}
const _: () = assert!(std::mem::size_of::<Properties>() == 48);
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmRegisterThumbnail(
        destination: *mut c_void,
        source: *mut c_void,
        thumbnail: *mut *mut c_void,
    ) -> i32;
    fn DwmQueryThumbnailSourceSize(thumbnail: *mut c_void, size: *mut Size) -> i32;
    fn DwmUpdateThumbnailProperties(thumbnail: *mut c_void, properties: *const Properties) -> i32;
    fn DwmUnregisterThumbnail(thumbnail: *mut c_void) -> i32;
}
#[link(name = "gdk-3")]
unsafe extern "C" {
    fn gdk_win32_window_get_handle(window: *mut c_void) -> *mut c_void;
}
struct Thumbnail(*mut c_void);
impl Drop for Thumbnail {
    fn drop(&mut self) {
        unsafe {
            DwmUnregisterThumbnail(self.0);
        }
    }
}

// DWM renders the preview; no capture/encode pipeline runs in the selector.
pub fn attach(area: &gtk::DrawingArea, dialog: &gtk::Dialog, source: u64) {
    let thumbnail = Rc::new(RefCell::new(None::<Thumbnail>));
    let state = thumbnail.clone();
    let parent = dialog.downgrade();
    area.connect_map(move |area| {
        let Some(dialog) = parent.upgrade() else {
            return;
        };
        let Some(window) = dialog.window() else {
            return;
        };
        unsafe {
            let destination = gdk_win32_window_get_handle(window.as_ptr().cast());
            let mut handle = std::ptr::null_mut();
            let result =
                DwmRegisterThumbnail(destination, source as usize as *mut c_void, &mut handle);
            if result >= 0 && !handle.is_null() {
                *state.borrow_mut() = Some(Thumbnail(handle));
                area.queue_draw();
            } else {
                super::diagnostics::event!(
                    "source-preview-unavailable",
                    serde_json::json!({"hresult":format!("0x{:08x}", result as u32)}),
                );
                area.set_tooltip_text(Some(
                    "Prévia indisponível. Confira se a janela continua aberta.",
                ));
            }
        }
    });
    let state = thumbnail.clone();
    area.connect_unmap(move |_| {
        state.borrow_mut().take();
    });
    let parent = dialog.downgrade();
    area.connect_draw(move |area, _| {
        if let Some(thumbnail) = thumbnail.borrow().as_ref()
            && let Some(dialog) = parent.upgrade()
            && let Some((x, y)) = area.translate_coordinates(&dialog, 0, 0)
        {
            let mut size = Size::default();
            unsafe {
                if DwmQueryThumbnailSourceSize(thumbnail.0, &mut size) >= 0
                    && size.width > 0
                    && size.height > 0
                {
                    let factor = area.scale_factor();
                    let scale = (area.allocated_width() as f64 / size.width as f64)
                        .min(area.allocated_height() as f64 / size.height as f64);
                    let w = (size.width as f64 * scale) as i32;
                    let h = (size.height as f64 * scale) as i32;
                    let left = x + (area.allocated_width() - w) / 2;
                    let top = y + (area.allocated_height() - h) / 2;
                    let properties = Properties {
                        flags: 1 | 4 | 8 | 16,
                        destination: Rect {
                            left: left * factor,
                            top: top * factor,
                            right: (left + w) * factor,
                            bottom: (top + h) * factor,
                        },
                        source: Rect::default(),
                        opacity: 255,
                        visible: 1,
                        client_only: 0,
                    };
                    DwmUpdateThumbnailProperties(thumbnail.0, &properties);
                }
            }
        }
        gtk::glib::Propagation::Proceed
    });
}
