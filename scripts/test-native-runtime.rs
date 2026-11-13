//! Links the ordinary library, without cfg(test), so diagnostic-only behavior cannot hide regressions.
use gst::prelude::*;
use sala::client::{media::Peer, video};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[test]
fn ordinary_receiver_counts_decoded_video_for_the_startup_watchdog() {
    gst::init().unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(128);
    let peer = Peer::new(1, 2, 3, false, false, tx).unwrap();
    let source = gst::parse_bin_from_description(
        "videotestsrc is-live=true num-buffers=90 ! video/x-raw,width=320,height=180,framerate=30/1 ! videoconvert ! openh264enc ! h264parse ! rtph264pay pt=96", true,
    ).unwrap();
    let rtc = peer.rtc.clone();
    source
        .get_static_pad("src")
        .unwrap()
        .add_probe(gst::PadProbeType::BUFFER, move |pad, _| {
            // Supply a real local RTP pad at the WebRTC boundary; no network or desktop is needed.
            rtc.emit("pad-added", &[pad]).unwrap();
            gst::PadProbeReturn::Remove
        });
    peer.pipeline.add(&source).unwrap();
    source.sync_state_with_parent().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while peer.video_frames.load(Ordering::Relaxed) < 60 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        if let Some(sala::client::Command::MediaError(_, _, _, _, error)) = rx.try_iter().next() {
            panic!("Receiver failed: {error}");
        }
    }
    assert!(
        peer.video_frames.load(Ordering::Relaxed) >= 60,
        "The normal client must count decoded video for its 30-second watchdog"
    );
    assert!(
        video::sample(3).is_some(),
        "The application did not receive the image"
    );
}
