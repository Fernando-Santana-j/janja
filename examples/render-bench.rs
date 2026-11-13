//! Headless Cairo scaling benchmark; never linked into the distributed client.
use gtk::cairo::{Context, Filter, Format, ImageSurface};
use std::time::Instant;

fn main() {
    for (width, height, target_width, target_height) in [
        (1920, 1080, 960, 540),
        (1920, 1080, 1280, 720),
        (1920, 1080, 2560, 1440),
        (1920, 1080, 3840, 2160),
        (3840, 2160, 960, 540),
    ] {
        let mut source = ImageSurface::create(Format::Rgb24, width, height).unwrap();
        for (index, pixel) in source
            .data()
            .unwrap()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            pixel.copy_from_slice(&((index as u32).wrapping_mul(0x10203) & 0xffffff).to_ne_bytes());
        }
        let target = ImageSurface::create(Format::Rgb24, target_width, target_height).unwrap();
        let cr = Context::new(&target).unwrap();
        cr.scale(
            target_width as f64 / width as f64,
            target_height as f64 / height as f64,
        );
        cr.set_source_surface(&source, 0., 0.).unwrap();
        for repetition in 0..3 {
            let filters = if repetition % 2 == 0 {
                [Filter::Good, Filter::Bilinear, Filter::Nearest]
            } else {
                [Filter::Nearest, Filter::Bilinear, Filter::Good]
            };
            for filter in filters {
                cr.source().set_filter(filter);
                cr.paint().unwrap();
                let started = Instant::now();
                for _ in 0..40 {
                    cr.paint().unwrap();
                }
                println!(
                    "{}",
                    serde_json::json!({
                        "source": [width, height], "target": [target_width, target_height],
                        "filter": format!("{filter:?}"), "repetition": repetition,
                        "ms_per_frame": started.elapsed().as_secs_f64() * 1000. / 40.,
                    })
                );
            }
        }
    }
}
