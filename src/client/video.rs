#[cfg(test)]
use gst::prelude::*;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
static SINKS: OnceLock<Mutex<HashMap<u64, (u64, gst_app::AppSink)>>> = OnceLock::new();
fn sinks() -> &'static Mutex<HashMap<u64, (u64, gst_app::AppSink)>> {
    SINKS.get_or_init(Mutex::default)
}
pub fn register(id: u64, generation: u64, sink: gst_app::AppSink) {
    let mut sinks = sinks().lock().unwrap();
    if sinks
        .get(&id)
        .is_some_and(|(current, _)| *current > generation)
    {
        return;
    }
    sinks.insert(id, (generation, sink));
}
pub fn remove(id: u64, generation: u64) {
    let mut sinks = sinks().lock().unwrap();
    if sinks.get(&id).is_some_and(|(g, _)| *g == generation) {
        sinks.remove(&id);
    }
}
pub fn sample(id: u64) -> Option<gst::Sample> {
    let sink = sinks().lock().ok()?.get(&id)?.1.clone();
    sink.try_pull_sample(gst::ClockTime::from_nseconds(0))
}
struct Mapping {
    _frame: gst_video::VideoFrame<gst_video::video_frame::Readable>,
}
static FRAME_DATA: gtk::cairo::UserDataKey<Mapping> = gtk::cairo::UserDataKey::new();
pub(super) struct Frame {
    surface: gtk::cairo::ImageSurface,
}
impl Frame {
    pub fn new(sample: &gst::Sample) -> Option<Self> {
        let info = gst_video::VideoInfo::from_caps(sample.get_caps()?).ok()?;
        let expected = if cfg!(target_endian = "little") {
            gst_video::VideoFormat::Bgrx
        } else {
            gst_video::VideoFormat::Xrgb
        };
        if info.format() != expected {
            return None;
        }
        let frame =
            gst_video::VideoFrame::from_buffer_readable(sample.get_buffer()?.to_owned(), &info)
                .ok()?;
        let data = frame.plane_data(0).ok()?;
        let stride = frame.plane_stride()[0];
        if stride < info.width() as i32 * 4
            || data.len() < (stride as usize).checked_mul(info.height() as usize)?
        {
            return None;
        }
        // Cairo only reads this private source surface; its user data owns the map until all Cairo references are gone.
        let surface = unsafe {
            gtk::cairo::ImageSurface::create_for_data_unsafe(
                data.as_ptr().cast_mut(),
                gtk::cairo::Format::Rgb24,
                info.width() as i32,
                info.height() as i32,
                stride,
            )
            .ok()?
        };
        surface
            .set_user_data(&FRAME_DATA, std::rc::Rc::new(Mapping { _frame: frame }))
            .ok()?;
        Some(Self { surface })
    }
    pub fn width(&self) -> i32 {
        self.surface.width()
    }
    pub fn height(&self) -> i32 {
        self.surface.height()
    }
    pub fn paint(
        &self,
        cr: &gtk::cairo::Context,
        width: f64,
        height: f64,
        fill: bool,
    ) -> Result<(), gtk::cairo::Error> {
        cr.save()?;
        let result = (|| {
            let (horizontal, vertical) =
                (width / self.width() as f64, height / self.height() as f64);
            let scale = if fill {
                horizontal.max(vertical)
            } else {
                horizontal.min(vertical)
            };
            cr.rectangle(0., 0., width, height);
            cr.clip();
            cr.translate(
                (width - self.width() as f64 * scale) / 2.,
                (height - self.height() as f64 * scale) / 2.,
            );
            cr.scale(scale, scale);
            cr.set_source_surface(&self.surface, 0., 0.)?;
            cr.paint()
        })();
        let restored = cr.restore();
        result.and(restored)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fullscreen_fills_viewport_without_letterboxing_or_stretching() {
        gst::init().unwrap();
        let format = if cfg!(target_endian = "little") {
            gst_video::VideoFormat::Bgrx
        } else {
            gst_video::VideoFormat::Xrgb
        };
        let info = gst_video::VideoInfo::builder(format, 16, 8)
            .build()
            .unwrap();
        let mut pixels = Vec::new();
        for _ in 0..8 {
            for x in 0..16 {
                let rgb = if x < 4 {
                    0xff0000u32
                } else if x < 12 {
                    0x00ff00u32
                } else {
                    0x0000ffu32
                };
                pixels.extend_from_slice(&rgb.to_ne_bytes());
            }
        }
        let sample = gst::Sample::builder()
            .caps(&info.to_caps().unwrap())
            .buffer(&gst::Buffer::from_slice(pixels))
            .build();
        let frame = Frame::new(&sample).unwrap();
        let mut target = gtk::cairo::ImageSurface::create(gtk::cairo::Format::Rgb24, 8, 8).unwrap();
        let cr = gtk::cairo::Context::new(&target).unwrap();
        cr.set_source_rgb(0.078, 0.09, 0.11);
        cr.paint().unwrap();
        frame.paint(&cr, 8., 8., true).unwrap();
        drop(cr);
        for pixel in target.data().unwrap().as_chunks::<4>().0 {
            assert_eq!(
                u32::from_ne_bytes(*pixel) & 0xffffff,
                0x00ff00,
                "Fullscreen left bars or distorted the selected region"
            );
        }
    }
    #[test]
    fn render_preserves_pixels_stride_and_buffer_lifetime() {
        gst::init().unwrap();
        let format = if cfg!(target_endian = "little") {
            gst_video::VideoFormat::Bgrx
        } else {
            gst_video::VideoFormat::Xrgb
        };
        let info = gst_video::VideoInfo::builder(format, 3, 2).build().unwrap();
        let mut buffer = gst::Buffer::from_slice(vec![0x55u8; 32]);
        gst_video::VideoMeta::add_full(
            buffer.get_mut().unwrap(),
            gst_video::VideoFrameFlags::empty(),
            format,
            3,
            2,
            &[0],
            &[16],
        )
        .unwrap();
        {
            let mut bytes = buffer.get_mut().unwrap().map_writable().unwrap();
            for row in 0..2 {
                for col in 0..3 {
                    let color = if cfg!(target_endian = "little") {
                        [21, 87, 199, 0]
                    } else {
                        [0, 199, 87, 21]
                    };
                    bytes.as_mut_slice()[row * 16 + col * 4..row * 16 + col * 4 + 4]
                        .copy_from_slice(&color);
                }
            }
        }
        let sample = gst::Sample::builder()
            .caps(&info.to_caps().unwrap())
            .buffer(&buffer)
            .build();
        let frame = Frame::new(&sample).unwrap();
        drop(sample);
        drop(buffer);
        assert_eq!((frame.width(), frame.height()), (3, 2));
        let mut target = gtk::cairo::ImageSurface::create(gtk::cairo::Format::Rgb24, 3, 2).unwrap();
        let cr = gtk::cairo::Context::new(&target).unwrap();
        frame.paint(&cr, 3., 2., false).unwrap();
        drop(cr);
        drop(frame);
        let data = target.data().unwrap();
        let expected = if cfg!(target_endian = "little") {
            [21, 87, 199]
        } else {
            [255, 199, 87]
        };
        assert_eq!(&data[0..3], &expected);
        assert_eq!(&data[12..15], &expected);
    }
    #[test]
    fn stale_cleanup_preserves_replacement_and_handles_padded_rgb() {
        gst::init().unwrap();
        let sink = gst::ElementFactory::make("appsink", None)
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap();
        register(987, 2, sink.clone());
        register(987, 1, sink);
        assert_eq!(sinks().lock().unwrap().get(&987).unwrap().0, 2);
        remove(987, 1);
        assert!(sinks().lock().unwrap().contains_key(&987));
        remove(987, 2);
    }
}
