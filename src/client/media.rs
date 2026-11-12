use super::{
    Command, Commands, Settings,
    adaptive::{Controller, Jitter, LossReports, MIN_JITTER_MS, Support},
};
use gst::prelude::*;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

type Targets = Arc<Mutex<HashMap<u64, (gst_app::AppSrc, gst::Pipeline)>>>;
const AUDIO_ENCODER: &str = "opusenc bitrate=96000 audio-type=generic frame-size=10 inband-fec=true packet-loss-percentage=5";
// Keep Opus away from the dynamically allocated H.264 RTX payload (usually 97).
const OPUS_PAYLOAD: u32 = 111;
#[derive(Default)]
struct AudioLevel(Mutex<Option<u16>>);
impl AudioLevel {
    fn observe(&self, buffer: &gst::BufferRef) {
        if buffer.get_size() < 2 {
            return;
        }
        let Ok(data) = buffer.map_readable() else {
            return;
        };
        // opusenc/opusdec negotiate native signed 16-bit PCM at these probes.
        let Some(peak) = data
            .as_slice()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&s| i16::from_ne_bytes(s).unsigned_abs())
            .max()
        else {
            return;
        };
        let mut current = self.0.lock().unwrap();
        *current = Some(current.unwrap_or(0).max(peak));
    }
    #[cfg(any(test, feature = "diagnostics"))]
    fn peak(&self) -> f64 {
        self.0.lock().unwrap().unwrap_or(0) as f64 / 32768.0
    }
    fn status(&self) -> &'static str {
        match self.0.lock().unwrap().take() {
            None => "sem dados",
            Some(0) => "silêncio",
            Some(_) => "sinal",
        }
    }
}
fn watch_audio(pad: &gst::Pad, level: Arc<AudioLevel>) {
    pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
        if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data {
            level.observe(buffer);
        }
        gst::PadProbeReturn::Ok
    });
}
pub struct Capture {
    pub pipeline: gst::Pipeline,
    video: Targets,
    audio: Targets,
    pub has_audio: bool,
    pub encoder: String,
    pub frames: Arc<AtomicU64>,
    pub raw_frames: Arc<AtomicU64>,
    pub encoder_input: Arc<AtomicU64>,
    pub encoded_bytes: Arc<AtomicU64>,
    audio_frames: Arc<AtomicU64>,
    audio_level: Arc<AudioLevel>,
    pub settings: Settings,
    controller: Controller,
    overruns: Arc<AtomicU64>,
    #[cfg(target_os = "linux")]
    _portal: Option<super::portal::Session>,
}
fn pipeline(text: &str) -> Result<gst::Pipeline, String> {
    let element = gst::parse_launch(text).map_err(|e| e.to_string())?;
    let pipeline = match element.downcast::<gst::Pipeline>() {
        Ok(pipeline) => pipeline,
        Err(element) => {
            let pipeline = gst::Pipeline::new(None);
            pipeline.add(&element).map_err(|e| e.to_string())?;
            pipeline
        }
    };
    pipeline.use_clock(Some(&gst::SystemClock::obtain()));
    Ok(pipeline)
}
fn pipe_error(p: &gst::Pipeline) -> Option<String> {
    let bus = p.get_bus()?;
    let mut latency_changed = false;
    let mut error = None;
    while let Some(message) = bus.pop() {
        match message.view() {
            gst::MessageView::Latency(_) => latency_changed = true,
            gst::MessageView::Error(e) => {
                error = Some(format!(
                    "{}: {} ({})",
                    e.get_src()
                        .map(|v| v.get_path_string().to_string())
                        .unwrap_or_default(),
                    e.get_error(),
                    e.get_debug().unwrap_or_default()
                ));
            }
            gst::MessageView::Eos(_) => {
                error.get_or_insert_with(|| "A fonte de mídia foi encerrada".into());
            }
            _ => (),
        }
    }
    if latency_changed && error.is_none() {
        if let Err(_e) = p.recalculate_latency() {
            super::diagnostics::event!(
                "latency-error",
                json!({"pipeline":p.get_name().to_string(),"error":_e.to_string()}),
            );
        } else {
            super::diagnostics::event!(
                "latency-updated",
                json!({"pipeline":p.get_name().to_string()}),
            );
        }
    }
    if error.is_some() {
        super::diagnostics::snapshot(p);
    }
    error
}

fn sample_time(
    sample: &gst::SampleRef,
    source_base: gst::ClockTime,
    destination_base: gst::ClockTime,
) -> gst::ClockTime {
    let time = (|| {
        let segment = sample.get_segment()?;
        let running = segment
            .downcast_ref::<gst::ClockTime>()?
            .to_running_time(sample.get_buffer()?.get_pts());
        running
            .0?
            .checked_add(source_base.0?)?
            .checked_sub(destination_base.0?)
    })();
    gst::ClockTime(time)
}
fn meter(
    pad: &gst::Pad,
    _stage: &'static str,
    frames: Arc<AtomicU64>,
    bytes: Option<Arc<AtomicU64>>,
) {
    pad.add_probe(gst::PadProbeType::BUFFER | gst::PadProbeType::EVENT_DOWNSTREAM, move |_pad, info| {
        match &info.data {
            Some(gst::PadProbeData::Buffer(buffer)) => {
                frames.fetch_add(1, Ordering::Relaxed);
                if let Some(bytes) = &bytes { bytes.fetch_add(buffer.get_size() as u64, Ordering::Relaxed); }
            }
            Some(gst::PadProbeData::Event(event)) => if let gst::EventView::Caps(_caps) = event.view() {
                super::diagnostics::event!("caps", json!({"stage":_stage,"pad":_pad.get_path_string().to_string(),"caps":super::diagnostics::safe_caps(_caps.get_caps())}));
            },
            _ => (),
        }
        gst::PadProbeReturn::Ok
    });
}
fn protect_h264_references(app: &gst_app::AppSrc) {
    let waiting = AtomicBool::new(true);
    let requested = AtomicBool::new(false);
    app.get_static_pad("src").unwrap().add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
        let Some(gst::PadProbeData::Buffer(buffer)) = &info.data else {
            return gst::PadProbeReturn::Ok;
        };
        // appsrc marks DISCONT when its bounded queue evicts a compressed reference.
        if buffer.get_flags().contains(gst::BufferFlags::DISCONT) {
            waiting.store(true, Ordering::Relaxed);
            requested.store(false, Ordering::Relaxed);
        }
        if !buffer.get_flags().contains(gst::BufferFlags::DELTA_UNIT) {
            waiting.store(false, Ordering::Relaxed);
            requested.store(false, Ordering::Relaxed);
        }
        if waiting.load(Ordering::Relaxed) {
            if !requested.swap(true, Ordering::Relaxed) {
                pad.send_event(gst_video::UpstreamForceKeyUnitEvent::builder().build());
                super::diagnostics::event!("h264-recovery", json!({"pad":pad.get_path_string().to_string(),"reason":"missing-reference"}));
            }
            return gst::PadProbeReturn::Drop;
        }
        gst::PadProbeReturn::Ok
    });
}

fn attach(
    p: &gst::Pipeline,
    name: &str,
    targets: Targets,
    count: Arc<AtomicU64>,
) -> Result<(), String> {
    let sink = p
        .get_by_name(name)
        .ok_or("Saída de captura ausente")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| "Saída de captura inválida")?;
    let source = p.downgrade();
    sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let Some(buffer) = sample.get_buffer() else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let Some(source) = source.upgrade() else {
                    return Err(gst::FlowError::Eos);
                };
                count.fetch_add(1, Ordering::Relaxed);
                let source_base = source.get_base_time();
                let targets: Vec<_> = targets.lock().unwrap().values().cloned().collect();
                for (app, destination) in targets {
                    let mut buffer = buffer.to_owned();
                    let data = buffer.make_mut();
                    // Rebase the shared capture clock for each peer without copying encoded payload.
                    let pts = sample_time(&sample, source_base, destination.get_base_time());
                    if pts.is_none() {
                        continue;
                    }
                    data.set_pts(pts);
                    data.set_dts(gst::CLOCK_TIME_NONE);
                    app.set_caps(sample.get_caps().map(|v| v.to_owned()).as_ref());
                    if let Err(_error) = app.push_buffer(buffer) {
                        super::diagnostics::event!("bridge-push-error", json!({"app":app.get_path_string().to_string(),"error":format!("{_error:?}")}));
                    }
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    Ok(())
}
fn encoders(s: &Settings) -> Vec<String> {
    let threads = codec_threads();
    let mut result = vec![];
    #[cfg(target_os = "windows")]
    if gst::ElementFactory::find("nvd3d11h264enc").is_some() {
        result.push(format!("nvd3d11h264enc name=encoder bitrate={} gop-size={} bframes=0 zerolatency=true rc-mode=cbr", s.bitrate, s.fps));
    }
    for (name, options) in [
        (
            "nvh264enc",
            format!(
                "bitrate={} gop-size={} bframes=0 zerolatency=true rc-mode=cbr",
                s.bitrate, s.fps
            ),
        ),
        (
            "qsvh264enc",
            format!("bitrate={} gop-size={} b-frames=0", s.bitrate, s.fps),
        ),
        (
            "vah264enc",
            format!("bitrate={} key-int-max={} b-frames=0", s.bitrate, s.fps),
        ),
        (
            "amfh264enc",
            format!("bitrate={} gop-size={} b-frames=0", s.bitrate, s.fps),
        ),
        (
            "mfh264enc",
            format!("bitrate={} low-latency=true", s.bitrate),
        ),
        (
            "x264enc",
            format!(
                "bitrate={} key-int-max={} tune=zerolatency speed-preset=ultrafast bframes=0 threads={threads}",
                s.bitrate, s.fps
            ),
        ),
        (
            "openh264enc",
            format!(
                "bitrate={} max-bitrate={} gop-size={} complexity=low rate-control=bitrate enable-frame-skip=true multi-thread={threads}",
                s.bitrate * 1000,
                s.bitrate * 1000,
                s.fps
            ),
        ),
    ] {
        if gst::ElementFactory::find(name).is_some() {
            result.push(format!("{name} name=encoder {options}"));
        }
    }
    result
}
fn thread_budget(cores: usize) -> usize {
    cores.saturating_sub(1).clamp(1, 4)
}
fn codec_threads() -> usize {
    thread_budget(
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(2),
    )
}

pub fn recommend(requested: &Settings) -> Result<(Settings, String), String> {
    requested.validate()?;
    let mut tested = Vec::new();
    let mut failures = Vec::new();
    for (height, fps) in [
        (requested.height, requested.fps),
        (1080, 30),
        (720, 30),
        (720, 15),
    ] {
        if height > requested.height || fps > requested.fps || tested.contains(&(height, fps)) {
            continue;
        }
        tested.push((height, fps));
        let probe = Settings {
            height,
            fps,
            audio: false,
            source: 0,
            ..requested.clone()
        };
        let capture = match Capture::start(&probe, "videotestsrc is-live=true pattern=smpte", "") {
            Ok(capture) => capture,
            Err(error) => {
                failures.push(error);
                continue;
            }
        };
        let start = capture.frames.load(Ordering::Relaxed);
        let clock = std::time::Instant::now();
        std::thread::sleep(Duration::from_millis(1500));
        if let Some(error) = capture.error() {
            return Err(error);
        }
        let measured =
            (capture.frames.load(Ordering::Relaxed) - start) as f64 / clock.elapsed().as_secs_f64();
        super::diagnostics::event!(
            "quality-probe",
            json!({"height":height,"target_fps":fps,"encoded_fps":measured,"encoder":capture.encoder,"codec_threads":codec_threads()}),
        );
        if measured >= fps as f64 * 0.9 {
            let profile = Settings {
                height,
                fps,
                source: 0,
                ..requested.clone()
            };
            return Ok((
                profile,
                format!(
                    "Perfil medido: {height}p / {fps} FPS / {}. Valide também com sua tela em movimento.",
                    capture.encoder
                ),
            ));
        }
    }
    Err(if let Some(error) = failures.last() {
        format!("Não foi possível medir a codificação: {error}")
    } else {
        "O teste local não sustentou 720p/15 FPS. Feche tarefas pesadas e tente novamente.".into()
    })
}

#[link(name = "gstaudio-1.0")]
unsafe extern "C" {
    fn gst_audio_decoder_set_latency(decoder: *mut std::ffi::c_void, min: u64, max: u64);
}
#[link(name = "opus")]
unsafe extern "C" {
    fn opus_packet_get_nb_samples(data: *const u8, len: i32, rate: i32) -> i32;
}

fn configure_opus_decoder(element: &gst::Element) {
    let Some(pad) = element.get_static_pad("sink") else {
        return;
    };
    let previous = AtomicU64::new(120_000_000);
    let next_pts = AtomicU64::new(u64::MAX);
    pad.add_probe(
        gst::PadProbeType::BUFFER | gst::PadProbeType::EVENT_DOWNSTREAM,
        move |pad, info| {
            if let Some(gst::PadProbeData::Event(event)) = &info.data {
                match event.view() {
                    gst::EventView::Gap(gap) => {
                        let (pts, duration) = gap.get();
                        if let (Some(pts), Some(duration)) = (pts.0, duration.0) {
                            next_pts.store(pts.saturating_add(duration), Ordering::Relaxed);
                        }
                    }
                    gst::EventView::StreamStart(_)
                    | gst::EventView::Segment(_)
                    | gst::EventView::Caps(_)
                    | gst::EventView::FlushStart(_)
                    | gst::EventView::FlushStop(_) => {
                        next_pts.store(u64::MAX, Ordering::Relaxed);
                        previous.store(120_000_000, Ordering::Relaxed);
                    }
                    _ => (),
                }
            }
            if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data
                && let Ok(bytes) = buffer.map_readable()
                && let Ok(length) = i32::try_from(bytes.len())
                && length > 0
            {
                // SAFETY: libopus only inspects this readable packet, bounded by its actual length.
                let samples = unsafe {
                    opus_packet_get_nb_samples(bytes.as_slice().as_ptr(), length, 48_000)
                };
                if (1..=5760).contains(&samples) {
                    let duration = samples as u64 * 1_000_000_000 / 48_000;
                    if let Some(pts) = buffer.get_pts().0 {
                        let expected = next_pts.load(Ordering::Relaxed);
                        let missing = pts.saturating_sub(expected);
                        // A full jitterbuffer can evict its own loss event; recover short holes once.
                        if expected != u64::MAX && missing > duration / 2 && missing <= 120_000_000
                        {
                            let _ = pad.send_event(gst::event::Gap::new(
                                gst::ClockTime::from_nseconds(expected),
                                gst::ClockTime::from_nseconds(missing),
                            ));
                        }
                        next_pts.store(pts.saturating_add(duration), Ordering::Relaxed);
                    } else {
                        next_pts.store(u64::MAX, Ordering::Relaxed);
                    }
                    if previous.swap(duration, Ordering::Relaxed) != duration
                        && let Some(decoder) = pad.get_parent_element()
                    {
                        use glib_old::translate::ToGlibPtr;
                        let pointer: *mut gst::gst_sys::GstElement = decoder.to_glib_none().0;
                        // SAFETY: this pad belongs to the live opusdec (GstAudioDecoder); the setter is MT-safe.
                        unsafe {
                            gst_audio_decoder_set_latency(pointer.cast(), duration, duration);
                        }
                    }
                }
            }
            gst::PadProbeReturn::Ok
        },
    );
}

fn configure_receiver_element(element: &gst::Element) {
    if element
        .get_factory()
        .is_some_and(|f| f.get_name() == "rtpsession")
    {
        // Feedback must arrive before the low-latency jitterbuffer expires missing references.
        element.set_property_from_str("rtcp-min-interval", "50000000");
    }
    if element
        .get_factory()
        .is_some_and(|f| f.get_name() == "rtpjitterbuffer")
    {
        element.set_property_from_str("drop-on-latency", "true");
        element.set_property_from_str("do-lost", "true");
        // Detect sequence gaps without requesting every predicted audio packet before it arrives.
        element.set_property_from_str("rtx-next-seqnum", "false");
    }
    if element
        .get_factory()
        .is_some_and(|f| f.get_name() == "avdec_h264")
    {
        element.set_property_from_str("max-threads", &codec_threads().to_string());
        // Frame threading buffers several future frames; slice threading preserves live latency.
        element.set_property_from_str("thread-type", "slice");
    }
    if element.find_property("slave-method").is_some()
        && element.find_property("provide-clock").is_some()
    {
        // All peers use SystemClock; resample follows it without moving the playout pointer.
        element.set_property_from_str("slave-method", "resample");
        element.set_property_from_str("provide-clock", "false");
        element.set_property_from_str("buffer-time", "40000");
        element.set_property_from_str("latency-time", "10000");
        if element.find_property("low-latency").is_some() {
            element.set_property_from_str("low-latency", "true");
        }
        super::diagnostics::event!(
            "audio-clock-policy",
            json!({"factory":element.get_factory().map(|f|f.get_name().to_string()),"slave_method":"resample","provide_clock":false}),
        );
    }
}

impl Capture {
    pub fn new(s: &Settings) -> Result<Self, String> {
        s.validate()?;
        #[cfg(target_os = "linux")]
        let portal = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Some(super::portal::Session::open()?)
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let source = if let Some(session) = &portal {
            format!(
                "pipewiresrc fd={} path={} do-timestamp=true",
                session.fd(),
                session.node
            )
        } else {
            format!(
                "ximagesrc xid={} use-damage=false show-pointer=true",
                s.source
            )
        };
        #[cfg(target_os = "windows")]
        let source = if s.source == 0 {
            "d3d11screencapturesrc show-cursor=true".into()
        } else {
            format!(
                "d3d11screencapturesrc capture-api=wgc window-handle={} show-cursor=true",
                s.source
            )
        };
        let audio = if s.audio {
            audio_source(s)?
        } else {
            String::new()
        };
        let capture = Self::start(s, &source, &audio)?;
        #[cfg(target_os = "linux")]
        {
            let mut capture = capture;
            capture._portal = portal;
            Ok(capture)
        }
        #[cfg(target_os = "windows")]
        {
            Ok(capture)
        }
    }
    fn start(s: &Settings, source: &str, audio: &str) -> Result<Self, String> {
        let (factory, options) = source.split_once(' ').unwrap_or((source, ""));
        let source = format!("{factory} name=screen_source {options}");
        let mut failures = Vec::new();
        for encoder in encoders(s) {
            super::diagnostics::event!(
                "encoder-attempt",
                json!({"encoder":encoder,"settings":format!("{s:?}"),"source":source}),
            );
            let format = if encoder.starts_with("openh264enc") || encoder.starts_with("x264enc") {
                "I420"
            } else {
                "NV12"
            };
            let gpu_capture = source.starts_with("d3d11screencapturesrc")
                && cfg!(windows)
                && ["nvd3d11h264enc", "qsvh264enc", "amfh264enc"]
                    .iter()
                    .any(|name| encoder.starts_with(name));
            let raw = if gpu_capture {
                format!(
                    "{source} ! capsfilter name=rate_filter caps=video/x-raw(memory:D3D11Memory),framerate={}/1 ! queue name=capture_queue max-size-buffers=2 max-size-time=0 max-size-bytes=0 leaky=downstream ! d3d11convert ! capsfilter name=size_filter caps=video/x-raw(memory:D3D11Memory),format=NV12,width={},height={},pixel-aspect-ratio=1/1",
                    s.fps,
                    s.height * 16 / 9,
                    s.height
                )
            } else {
                format!(
                    "{source} ! video/x-raw,pixel-aspect-ratio=1/1 ! videorate drop-only=true ! capsfilter name=rate_filter caps=video/x-raw,framerate={}/1 ! queue name=capture_queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream ! videoconvertscale add-borders=true ! capsfilter name=size_filter caps=video/x-raw,format={format},width={},height={},pixel-aspect-ratio=1/1",
                    s.fps,
                    s.height * 16 / 9,
                    s.height
                )
            };
            let text = format!(
                "{raw} ! queue name=encoder_queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream ! {encoder} ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au ! appsink name=video sync=false max-buffers=2 drop=false {}",
                if s.audio {
                    format!(
                        "{audio} ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! identity name=capture_audio_meter ! {AUDIO_ENCODER} ! appsink name=audio sync=false max-buffers=4 drop=true enable-last-sample=false"
                    )
                } else {
                    String::new()
                }
            );
            let p = match pipeline(&text) {
                Ok(p) => p,
                Err(e) => {
                    failures.push(format!("{encoder}: {e}"));
                    continue;
                }
            };
            let capture = Self {
                pipeline: p,
                video: Arc::default(),
                audio: Arc::default(),
                has_audio: s.audio,
                encoder: encoder.split_whitespace().next().unwrap_or("H264").into(),
                frames: Arc::default(),
                raw_frames: Arc::default(),
                encoder_input: Arc::default(),
                encoded_bytes: Arc::default(),
                audio_frames: Arc::default(),
                audio_level: Arc::default(),
                settings: s.clone(),
                controller: Controller::default(),
                overruns: Arc::default(),
                #[cfg(target_os = "linux")]
                _portal: None,
            };
            for name in ["capture_queue", "encoder_queue"] {
                let overruns = capture.overruns.clone();
                capture
                    .pipeline
                    .get_by_name(name)
                    .ok_or("Fila de vídeo ausente")?
                    .connect("overrun", false, move |_| {
                        overruns.fetch_add(1, Ordering::Relaxed);
                        None
                    })
                    .map_err(|e| e.to_string())?;
            }
            let encoder_element = capture
                .pipeline
                .get_by_name("encoder")
                .ok_or("Encoder ausente")?;
            meter(
                &encoder_element
                    .get_static_pad("sink")
                    .ok_or("Encoder sem entrada")?,
                "encoder-input",
                capture.encoder_input.clone(),
                None,
            );
            meter(
                &encoder_element
                    .get_static_pad("src")
                    .ok_or("Encoder sem saída")?,
                "encoded",
                Arc::default(),
                Some(capture.encoded_bytes.clone()),
            );
            let source_element = capture
                .pipeline
                .get_by_name("screen_source")
                .ok_or("Captura ausente")?;
            meter(
                &source_element
                    .get_static_pad("src")
                    .ok_or("Captura sem saída")?,
                "capture",
                capture.raw_frames.clone(),
                None,
            );
            attach(
                &capture.pipeline,
                "video",
                capture.video.clone(),
                capture.frames.clone(),
            )?;
            if s.audio {
                watch_audio(
                    &capture
                        .pipeline
                        .get_by_name("capture_audio_meter")
                        .unwrap()
                        .get_static_pad("src")
                        .unwrap(),
                    capture.audio_level.clone(),
                );
                attach(
                    &capture.pipeline,
                    "audio",
                    capture.audio.clone(),
                    capture.audio_frames.clone(),
                )?;
            }
            if capture.pipeline.set_state(gst::State::Playing).is_err() {
                failures.push(format!(
                    "{encoder}: {}",
                    pipe_error(&capture.pipeline).unwrap_or("Falha ao iniciar captura".into())
                ));
                continue;
            }
            let (state, _, _) = capture.pipeline.get_state(gst::ClockTime::from_seconds(3));
            let error = pipe_error(&capture.pipeline);
            if state.is_err() || error.is_some() {
                failures.push(format!(
                    "{encoder}: {}",
                    error.unwrap_or("Falha na transição para Playing".into())
                ));
                continue;
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut failed = false;
            while capture.frames.load(Ordering::Relaxed) == 0
                || (s.audio && capture.audio_frames.load(Ordering::Relaxed) == 0)
            {
                if let Some(error) = capture.error() {
                    failures.push(format!("{encoder}: {error}"));
                    failed = true;
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    failures.push(format!(
                        "{encoder}: captura iniciou, mas não entregou vídeo/áudio"
                    ));
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if !failed
                && capture.frames.load(Ordering::Relaxed) > 0
                && (!s.audio || capture.audio_frames.load(Ordering::Relaxed) > 0)
            {
                super::diagnostics::event!(
                    "capture-ready",
                    json!({"encoder":encoder,"failures":failures}),
                );
                super::diagnostics::snapshot(&capture.pipeline);
                return Ok(capture);
            }
        }
        Err(if failures.is_empty() {
            "Nenhum codificador H.264 disponível".into()
        } else {
            failures.join("\n")
        })
    }
    pub fn add(&self, id: u64, peer: &Peer) {
        if let Some(app) = &peer.video {
            protect_h264_references(app);
            if let Some(pad) = app.get_static_pad("src") {
                let capture = self.pipeline.downgrade();
                pad.add_probe(gst::PadProbeType::EVENT_UPSTREAM, move |_, info| {
                    if let Some(gst::PadProbeData::Event(event)) = &info.data
                        && event
                            .get_structure()
                            .is_some_and(|s| s.get_name() == "GstForceKeyUnit")
                        && let Some(pipeline) = capture.upgrade()
                        && let Some(encoder) = pipeline.get_by_name("encoder")
                        && let Some(src) = encoder.get_static_pad("src")
                    {
                        src.send_event(gst_video::UpstreamForceKeyUnitEvent::builder().build());
                    }
                    gst::PadProbeReturn::Ok
                });
            }
            self.video
                .lock()
                .unwrap()
                .insert(id, (app.clone(), peer.pipeline.clone()));
        }
        if let Some(app) = &peer.audio {
            self.audio
                .lock()
                .unwrap()
                .insert(id, (app.clone(), peer.pipeline.clone()));
        }
    }
    pub fn remove(&self, id: u64) {
        self.video.lock().unwrap().remove(&id);
        self.audio.lock().unwrap().remove(&id);
    }
    fn support(&self) -> Support {
        let bitrate = self
            .pipeline
            .get_by_name("encoder")
            .and_then(|e| e.find_property("bitrate"))
            .is_some_and(|p| {
                use glib_old::translate::ToGlibPtr;
                let spec: *const gst::gobject_sys::GParamSpec = p.to_glib_none().0;
                // SAFETY: p owns the immutable GParamSpec; old GLib bindings truncate GStreamer's custom flags.
                unsafe { (*spec).flags & gst::gst_sys::GST_PARAM_MUTABLE_PLAYING as u32 != 0 }
            });
        Support {
            bitrate,
            // shortcut: live resizing is verified for software encoders; validate each GPU backend before enabling it.
            resize: matches!(self.encoder.as_str(), "x264enc" | "openh264enc"),
        }
    }
    fn retune(&mut self, next: Settings) -> Result<(), String> {
        next.validate()?;
        if next.bitrate != self.settings.bitrate {
            if !self.support().bitrate {
                return Err("Este encoder não ajusta a taxa durante a transmissão".into());
            }
            let encoder = self
                .pipeline
                .get_by_name("encoder")
                .ok_or("Encoder ausente")?;
            let value = if self.encoder == "openh264enc" {
                next.bitrate * 1000
            } else {
                next.bitrate
            };
            encoder
                .set_property("bitrate", &value)
                .map_err(|e| e.to_string())?;
            if self.encoder == "openh264enc" {
                encoder
                    .set_property("max-bitrate", &value)
                    .map_err(|e| e.to_string())?;
            }
        }
        for (name, changed) in [
            ("rate_filter", next.fps != self.settings.fps),
            ("size_filter", next.height != self.settings.height),
        ] {
            if !changed {
                continue;
            }
            if name == "size_filter" && !self.support().resize {
                return Err("Redimensionamento ao vivo indisponível neste encoder".into());
            }
            let filter = self
                .pipeline
                .get_by_name(name)
                .ok_or("Ajuste de vídeo ausente")?;
            let mut caps = filter
                .get_property("caps")
                .map_err(|e| e.to_string())?
                .get::<gst::Caps>()
                .map_err(|e| e.to_string())?
                .ok_or("Formato de vídeo ausente")?;
            let structure = caps
                .make_mut()
                .get_mut_structure(0)
                .ok_or("Formato de vídeo inválido")?;
            if name == "rate_filter" {
                structure.set("framerate", &gst::Fraction::new(next.fps as i32, 1));
            } else {
                structure.set("width", &((next.height * 16 / 9) as i32));
                structure.set("height", &(next.height as i32));
            }
            filter
                .set_property("caps", &caps)
                .map_err(|e| e.to_string())?;
        }
        self.settings = next;
        Ok(())
    }
    pub fn adapt(
        &mut self,
        elapsed: Duration,
        raw_fps: f64,
        encoded_fps: f64,
        network: bool,
    ) -> Result<Option<&'static str>, String> {
        let overruns = self.overruns.swap(0, Ordering::Relaxed);
        if !self.settings.adaptive {
            return Ok(None);
        }
        let local = encoded_fps < self.settings.fps as f64 * 0.8
            && (raw_fps >= self.settings.fps as f64 * 0.8 || overruns >= 2);
        let support = self.support();
        let Some((next, reason)) =
            self.controller
                .next(&self.settings, elapsed, local, network, support)
        else {
            return Ok(None);
        };
        self.retune(next)?;
        super::diagnostics::event!(
            "quality-adapted",
            json!({"reason":reason,"height":self.settings.height,"fps":self.settings.fps,"bitrate_kbps":self.settings.bitrate,"overruns":overruns})
        );
        Ok(Some(reason))
    }
    pub fn error(&self) -> Option<String> {
        pipe_error(&self.pipeline)
    }
    pub fn audio_status(&self) -> &'static str {
        if self.has_audio {
            self.audio_level.status()
        } else {
            "desligado"
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.video.lock().unwrap().clear();
        self.audio.lock().unwrap().clear();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

#[cfg(target_os = "linux")]
fn audio_source(s: &Settings) -> Result<String, String> {
    if s.exclude_discord {
        return Err("Exclusão do Discord ainda indisponível no Linux".into());
    }
    // PulseAudio's compatibility server also works on PipeWire desktops.
    Ok("pulsesrc device=@DEFAULT_MONITOR@ do-timestamp=true".into())
}
#[cfg(target_os = "windows")]
fn audio_source(s: &Settings) -> Result<String, String> {
    let source = gst::ElementFactory::make("wasapi2src", None).map_err(|e| e.to_string())?;
    let process_supported = source.find_property("loopback-mode").is_some()
        && source.find_property("loopback-target-pid").is_some();
    if !s.exclude_discord {
        return windows_loopback(process_supported, None);
    }
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile","-NonInteractive","-Command",
            "$ErrorActionPreference='Stop'; $p=@(Get-CimInstance Win32_Process -Filter \"Name='Discord.exe'\"); $ids=@($p.ProcessId); $p | Where-Object { $_.ParentProcessId -notin $ids } | Select-Object -First 1 -ExpandProperty ProcessId"])
        .output().map_err(|e| e.to_string())?;
    let pid = discord_pid(result.status.success(), &result.stdout)?;
    windows_loopback(process_supported, pid)
}

#[cfg(any(target_os = "windows", test))]
fn discord_pid(success: bool, output: &[u8]) -> Result<Option<u32>, String> {
    if !success {
        return Err("Não foi possível localizar o Discord. Tente novamente ou desative a exclusão de áudio.".into());
    }
    let text = std::str::from_utf8(output)
        .map_err(|_| "Resposta inválida ao localizar o Discord")?
        .trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u32>()
        .ok()
        .filter(|pid| *pid != 0)
        .map(Some)
        .ok_or_else(|| "Processo do Discord inválido".into())
}

#[cfg(test)]
#[test]
fn discord_can_be_closed_without_blocking_screen_sharing() {
    let absent = discord_pid(true, b"\r\n").unwrap();
    assert_eq!(absent, None);
    assert_eq!(
        windows_loopback(false, absent).unwrap(),
        "wasapi2src loopback=true low-latency=true"
    );
    assert!(
        !windows_loopback(true, absent)
            .unwrap()
            .contains("target-pid=0")
    );
    assert_eq!(discord_pid(true, b"1234\r\n").unwrap(), Some(1234));
    assert!(discord_pid(false, b"").is_err());
    assert!(discord_pid(true, b"0").is_err());
    assert!(discord_pid(true, b"not a pid").is_err());
}

#[cfg(any(target_os = "windows", test))]
fn windows_loopback(process_supported: bool, excluded_pid: Option<u32>) -> Result<String, String> {
    if process_supported {
        let pid = excluded_pid.unwrap_or_else(std::process::id);
        if pid == 0 {
            return Err("Processo de áudio inválido".into());
        }
        // Process loopback is independent of a render endpoint and excludes our own playback by default.
        Ok(format!(
            "wasapi2src loopback=true low-latency=true loopback-mode=exclude-process-tree loopback-target-pid={pid}"
        ))
    } else if excluded_pid.is_some() {
        Err("Exclusão de processos de áudio indisponível neste Windows".into())
    } else {
        Ok("wasapi2src loopback=true low-latency=true".into())
    }
}

#[cfg(target_endian = "little")]
const DISPLAY_SINK: &str = "video/x-raw,format=BGRx ! appsink name=display sync=true qos=true max-lateness=100000000 max-buffers=1 drop=true enable-last-sample=false";
#[cfg(target_endian = "big")]
const DISPLAY_SINK: &str = "video/x-raw,format=xRGB ! appsink name=display sync=true qos=true max-lateness=100000000 max-buffers=1 drop=true enable-last-sample=false";
fn receive_video(sink: &str) -> String {
    // GTK consumes CPU frames; DXVA readback dropped frames on the tested Windows GPU.
    let decoder = if cfg!(windows) {
        "avdec_h264"
    } else {
        "decodebin"
    };
    format!(
        "queue name=rtp_input max-size-time=100000000 max-size-bytes=0 max-size-buffers=0 ! rtph264depay name=video_depay wait-for-keyframe=true request-keyframe=true ! video/x-h264,alignment=au ! h264parse ! {decoder} ! videoconvert ! identity name=video_meter ! {sink}"
    )
}

fn receive_audio(sink: &str) -> String {
    format!(
        "queue name=rtp_input max-size-time=100000000 max-size-bytes=0 max-size-buffers=0 ! rtpopusdepay name=audio_depay ! opusdec name=audio_decoder plc=true use-inband-fec=true ! volume name=playback_volume ! identity name=audio_meter ! audioconvert ! audioresample ! {sink}"
    )
}

fn receive_bin(media: &str, headless: bool) -> Result<gst::Bin, String> {
    let chain = match media {
        "video" => receive_video(DISPLAY_SINK),
        "audio" => receive_audio(if headless {
            "fakesink name=audio_output sync=true signal-handoffs=true"
        } else {
            "autoaudiosink sync=true"
        }),
        _ => return Err("Tipo de mídia não suportado".into()),
    };
    let bin = gst::parse_bin_from_description(&chain, false).map_err(|e| e.to_string())?;
    bin.connect("deep-element-added", false, |values| {
        if let Ok(Some(element)) = values[2].get::<gst::Element>() {
            configure_receiver_element(&element);
        }
        None
    })
    .map_err(|e| e.to_string())?;
    for element in bin.iterate_recurse().into_iter().flatten() {
        configure_receiver_element(&element);
    }
    if let Some(decoder) = bin.get_by_name("audio_decoder") {
        // FEC holds one packet, rather than the maximum Opus packet length (120 ms).
        configure_opus_decoder(&decoder);
    }
    // decodebin also has an unlinked raw pad until negotiation; never auto-select it as RTP input.
    let target = bin
        .get_by_name("rtp_input")
        .and_then(|e| e.get_static_pad("sink"))
        .ok_or("Entrada RTP ausente")?;
    let ghost = gst::GhostPad::with_target(Some("sink"), &target).map_err(|e| e.to_string())?;
    bin.add_pad(&ghost).map_err(|e| e.to_string())?;
    Ok(bin)
}

pub struct Peer {
    pub pipeline: gst::Pipeline,
    pub rtc: gst::Element,
    video: Option<gst_app::AppSrc>,
    audio: Option<gst_app::AppSrc>,
    pub generation: u64,
    view: Option<u64>,
    volume: Arc<Mutex<u8>>,
    pub video_frames: Arc<AtomicU64>,
    pub audio_frames: Arc<AtomicU64>,
    audio_level: Arc<AudioLevel>,
    pub session: Option<String>,
    pub remote_ready: bool,
    loss: Arc<Mutex<LossReports>>,
    jitter: Mutex<Jitter>,
    stats_pending: Arc<AtomicBool>,
    pub pending: Vec<(u32, String, Option<String>)>,
    pub created: std::time::Instant,
}
impl Peer {
    pub fn new(
        epoch: u64,
        to: u64,
        stream: u64,
        sending: bool,
        audio: bool,
        tx: Commands,
    ) -> Result<Self, String> {
        Self::with_output(epoch, to, stream, sending, audio, tx, false)
    }
    fn with_output(
        epoch: u64,
        to: u64,
        stream: u64,
        sending: bool,
        audio: bool,
        tx: Commands,
        headless: bool,
    ) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT.fetch_add(1, Ordering::Relaxed);
        let volume = Arc::new(Mutex::new(100));
        let video_frames = Arc::new(AtomicU64::new(0));
        let audio_frames = Arc::new(AtomicU64::new(0));
        let audio_level = Arc::new(AudioLevel::default());
        let app = |name: &str, chain: &str| {
            let limits = if name == "a" {
                "max-buffers=12 max-time=120000000"
            } else {
                "max-buffers=2 max-time=0"
            };
            format!(
                "appsrc name={name} is-live=true format=time min-latency=0 max-latency=-1 {limits} max-bytes=0 leaky-type=downstream ! {chain} ! rtc. "
            )
        };
        let mut text =
            format!("webrtcbin name=rtc bundle-policy=max-bundle latency={MIN_JITTER_MS} ");
        if !headless {
            text.push_str("stun-server=stun://stun.cloudflare.com:3478 ");
        }
        if sending {
            let video_ssrc = (generation as u32).wrapping_mul(2).wrapping_add(1);
            let audio_ssrc = video_ssrc.wrapping_add(1);
            text.push_str(&app("v",&format!("video/x-h264,stream-format=byte-stream,alignment=au ! rtph264pay ssrc={video_ssrc} config-interval=-1 pt=96 aggregate-mode=zero-latency ! application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000,ssrc=(uint){video_ssrc}")));
            if audio {
                text.push_str(&app("a",&format!("audio/x-opus,rate=48000,channels=2,channel-mapping-family=0 ! rtpopuspay ssrc={audio_ssrc} pt={OPUS_PAYLOAD} ! application/x-rtp,media=audio,encoding-name=OPUS,payload={OPUS_PAYLOAD},clock-rate=48000,encoding-params=(string)2,sprop-stereo=(string)1,useinbandfec=(string)1,ssrc=(uint){audio_ssrc}")));
            }
        }
        let p = pipeline(&text)?;
        p.connect("deep-element-added", false, move |values| {
            if let Ok(Some(element)) = values[2].get::<gst::Element>() {
                configure_receiver_element(&element);
                super::diagnostics::event!("peer-element", json!({"peer":to,"stream":stream,"generation":generation,"path":element.get_path_string().to_string(),"factory":element.get_factory().map(|f|f.get_name().to_string())}));
            }
            None
        }).map_err(|e|e.to_string())?;
        let rtc = p.get_by_name("rtc").ok_or("WebRTC indisponível")?;
        let nack_tx = tx.clone();
        rtc.connect("on-new-transceiver", false, move |values| {
            if let Ok(Some(transceiver)) = values[1].get::<glib_old::Object>()
                && let Err(error) = transceiver.set_property("do-nack", &true)
            {
                let _ = nack_tx.try_send(Command::MediaError(
                    epoch,
                    to,
                    stream,
                    generation,
                    error.to_string(),
                ));
            }
            None
        })
        .map_err(|e| e.to_string())?;
        for pad in rtc.get_sink_pads() {
            if let Some(transceiver) = pad
                .get_property("transceiver")
                .map_err(|e| e.to_string())?
                .get::<glib_old::Object>()
                .map_err(|e| e.to_string())?
            {
                transceiver
                    .set_property("do-nack", &true)
                    .map_err(|e| e.to_string())?;
            }
        }
        let ice_tx = tx.clone();
        rtc.connect("on-ice-candidate", false, move |values| {
            if let (Ok(Some(index)), Ok(Some(candidate))) =
                (values[1].get::<u32>(), values[2].get::<String>())
            {
                let _ = ice_tx.try_send(Command::Signal(
                    epoch,
                    to,
                    stream,
                    generation,
                    json!({"candidate":{"sdpMLineIndex":index,"candidate":candidate}}),
                ));
            }
            None
        })
        .map_err(|e| e.to_string())?;
        if !sending {
            let weak = p.downgrade();
            let gain = volume.clone();
            let video_count = video_frames.clone();
            let audio_count = audio_frames.clone();
            let audio_signal = audio_level.clone();
            rtc.connect_pad_added(move |_, pad| {
                if pad.get_direction() != gst::PadDirection::Src {
                    return;
                }
                let Some(pipeline) = weak.upgrade() else {
                    return;
                };
                let result = (|| -> Result<(), String> {
                    let caps = pad.get_current_caps().ok_or("Mídia sem formato")?;
                    super::diagnostics::event!(
                        "incoming-rtp",
                        json!({"peer":to,"stream":stream,"caps":super::diagnostics::safe_caps(caps.as_ref())}),
                    );
                    let media = caps
                        .get_structure(0)
                        .ok_or("Formato vazio")?
                        .get::<String>("media")
                        .map_err(|e| e.to_string())?;
                    let bin = receive_bin(media.as_deref().unwrap_or(""), headless)?;
                    let (meter, count) = if media.as_deref() == Some("video") {
                        ("video_meter", video_count.clone())
                    } else {
                        ("audio_meter", audio_count.clone())
                    };
                    if let Some(element) = bin.get_by_name(meter) {
                        if media.as_deref() == Some("audio") {
                            watch_audio(&element.get_static_pad("src").ok_or("Medidor de áudio sem saída")?, audio_signal.clone());
                        }
                        if let Some(pad) = element.get_static_pad("src") {
                            self::meter(&pad, "receiver-output", Arc::default(), None);
                        }
                        element
                            .connect("handoff", false, move |_| {
                                count.fetch_add(1, Ordering::Relaxed);
                                None
                            })
                            .map_err(|e| e.to_string())?;
                    }
                    if !headless
                        && let Some(display) = bin
                            .get_by_name("display")
                            .and_then(|s| s.downcast::<gst_app::AppSink>().ok())
                    {
                        super::video::register(stream, generation, display);
                    }
                    pipeline.add(&bin).map_err(|e| e.to_string())?;
                    {
                        let gain = gain.lock().unwrap();
                        if let Some(volume) = bin.get_by_name("playback_volume") {
                            volume.set_property("volume", &(*gain as f64 / 100.0)).map_err(|e| e.to_string())?;
                        }
                    }
                    let sink = bin.get_static_pad("sink").ok_or("Saída sem pad")?;
                    pad.link(&sink).map_err(|e| e.to_string())?;
                    bin.sync_state_with_parent().map_err(|e| e.to_string())?;
                    Ok(())
                })();
                if let Err(e) = result {
                    let _ = tx.try_send(Command::MediaError(epoch, to, stream, generation, e));
                }
            });
        }
        let get_app = |name| {
            p.get_by_name(name)
                .and_then(|v| v.downcast::<gst_app::AppSrc>().ok())
        };
        let peer = Self {
            video: get_app("v"),
            audio: get_app("a"),
            pipeline: p,
            rtc,
            generation,
            view: (!sending && !headless).then_some(stream),
            volume,
            video_frames,
            audio_frames,
            audio_level,
            session: sending.then(|| format!("native-v2-{epoch}-{stream}-{generation}")),
            remote_ready: false,
            loss: Arc::default(),
            jitter: Mutex::default(),
            stats_pending: Arc::default(),
            pending: vec![],
            created: std::time::Instant::now(),
        };
        peer.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| e.to_string())?;
        Ok(peer)
    }
    pub fn tune_latency(&self, elapsed: Duration) {
        if self.video.is_some() {
            return;
        }
        let (mut late, mut jitter) = (0u64, 0u64);
        for element in self.pipeline.iterate_recurse().into_iter().flatten() {
            if element
                .get_factory()
                .is_some_and(|factory| factory.get_name() == "rtpjitterbuffer")
                && let Ok(value) = element.get_property("stats")
                && let Ok(Some(stats)) = value.get::<gst::Structure>()
            {
                late =
                    late.saturating_add(stats.get::<u64>("num-late").ok().flatten().unwrap_or(0));
                jitter = jitter.max(stats.get::<u64>("avg-jitter").ok().flatten().unwrap_or(0));
            }
        }
        let next = self.jitter.lock().unwrap().next(elapsed, late, jitter);
        if let Some(ms) = next {
            self.rtc
                .set_property("latency", &ms)
                .expect("WebRTC latency is a writable u32 property");
            let _ = self.pipeline.recalculate_latency();
            super::diagnostics::event!(
                "playback-latency",
                json!({"generation":self.generation,"buffer_ms":ms,"late_packets":late,"jitter_ns":jitter})
            );
        }
    }
    pub fn log_stats(&self) {
        #[cfg(any(test, feature = "diagnostics"))]
        {
            let generation = self.generation;
            super::diagnostics::event!(
                "peer-metrics",
                json!({"generation":generation,"direction":if self.video.is_some() {"send"} else {"receive"},"video_frames":self.video_frames.load(Ordering::Relaxed),"audio_buffers":self.audio_frames.load(Ordering::Relaxed),"volume":*self.volume.lock().unwrap(),"connection_state":self.rtc.get_property("connection-state").ok().and_then(|v|glib_old::EnumValue::from_value(&v).map(|e|e.get_nick().to_string()))}),
            );
        }
        if self.video.is_none() || self.stats_pending.swap(true, Ordering::Relaxed) {
            return;
        }
        let pending = self.stats_pending.clone();
        let loss = self.loss.clone();
        let promise = gst::Promise::with_change_func(move |reply| {
            if let Ok(Some(stats)) = reply {
                loss.lock()
                    .unwrap()
                    .observe(stats, std::time::Instant::now());
            }
            pending.store(false, Ordering::Relaxed);
        });
        if self
            .rtc
            .emit("get-stats", &[&None::<gst::Pad>, &promise])
            .is_err()
        {
            self.stats_pending.store(false, Ordering::Relaxed);
        }
    }
    pub fn connected(&self) -> bool {
        self.rtc
            .get_property("connection-state")
            .ok()
            .is_some_and(|value| {
                glib_old::EnumValue::from_value(&value)
                    .is_some_and(|state| state.get_nick() == "connected")
            })
    }
    pub fn congested(&self) -> bool {
        self.loss
            .lock()
            .unwrap()
            .congested(std::time::Instant::now())
    }
    pub fn set_volume(&self, percent: u8) -> Result<(), String> {
        if percent > 100 {
            return Err("Volume fora dos limites".into());
        }
        let mut current = self.volume.lock().unwrap();
        *current = percent;
        if let Some(volume) = self.pipeline.get_by_name("playback_volume") {
            volume
                .set_property("volume", &(percent as f64 / 100.0))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn audio_status(&self) -> &'static str {
        let status = self.audio_level.status();
        if *self.volume.lock().unwrap() == 0 {
            "silenciado"
        } else {
            status
        }
    }
    pub fn candidate(
        &mut self,
        index: u32,
        candidate: String,
        session: Option<String>,
    ) -> Result<(), String> {
        if index > 8 || candidate.len() > 4096 {
            return Err("Candidato ICE inválido".into());
        }
        if self.remote_ready {
            if session.is_some() && session != self.session {
                return Ok(());
            }
            self.rtc
                .emit("add-ice-candidate", &[&index, &candidate])
                .map_err(|e| e.to_string())?;
        } else {
            if self.pending.len() >= 64 {
                return Err("Excesso de candidatos ICE".into());
            }
            self.pending.push((index, candidate, session));
        }
        Ok(())
    }
    pub fn ready(&mut self) -> Result<(), String> {
        self.remote_ready = true;
        for (index, candidate, session) in std::mem::take(&mut self.pending) {
            self.candidate(index, candidate, session)?;
        }
        Ok(())
    }
    pub fn error(&self) -> Option<String> {
        if let Some(e) = pipe_error(&self.pipeline) {
            return Some(e);
        }
        if self.created.elapsed() > Duration::from_secs(30)
            && self.video.is_none()
            && self.video_frames.load(Ordering::Relaxed) == 0
        {
            return Some("Conexão sem imagem decodificada. Verifique o diagnóstico do transmissor e use a mesma versão nativa nos dois PCs.".into());
        }
        if self.created.elapsed() > Duration::from_secs(25) {
            let value = self.rtc.get_property("connection-state").ok()?;
            let enum_value = glib_old::EnumValue::from_value(&value)?;
            if !matches!(enum_value.get_nick(), "connected") {
                return Some(
                    "Não foi possível conectar diretamente. A rede pode exigir TURN.".into(),
                );
            }
        }
        None
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(id) = self.view {
            super::video::remove(id, self.generation);
        }
    }
}

/// Exercises the actual shared encoder bridge; no desktop, browser or server is needed.
#[cfg(any(test, feature = "diagnostics"))]
pub fn self_test(network: bool) -> Result<serde_json::Value, String> {
    test_stream(network, false)
}

#[cfg(any(test, feature = "diagnostics"))]
fn test_stream(network: bool, retune: bool) -> Result<serde_json::Value, String> {
    test_stream_case(network, retune, false)
}

#[cfg(any(test, feature = "diagnostics"))]
fn test_stream_case(
    network: bool,
    retune: bool,
    packet_loss: bool,
) -> Result<serde_json::Value, String> {
    if network {
        std::net::UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| format!("O sistema bloqueou o teste UDP: {e}"))?;
    }
    gst::init().map_err(|e| e.to_string())?;
    let settings = Settings {
        height: 1080,
        fps: 30,
        bitrate: 8000,
        ..Settings::default()
    };
    let mut capture = Capture::start(
        &settings,
        if packet_loss {
            "videotestsrc is-live=true pattern=smpte"
        } else {
            "videotestsrc is-live=true pattern=ball"
        },
        "audiotestsrc is-live=true volume=0.05",
    )?;
    let caps = capture
        .pipeline
        .get_by_name("screen_source")
        .and_then(|s| s.get_static_pad("src"))
        .and_then(|p| p.get_current_caps())
        .ok_or("Formato da captura ausente")?;
    let par = caps
        .get_structure(0)
        .ok_or("Captura sem estrutura")?
        .get::<gst::Fraction>("pixel-aspect-ratio")
        .map_err(|e| e.to_string())?;
    if par != Some(gst::Fraction::new(1, 1)) {
        return Err("Captura negociou proporção de pixel inválida".into());
    }
    let start = std::time::Instant::now();
    let (tx, rx) = std::sync::mpsc::sync_channel(128);
    let (mut sender, mut receiver) = if network {
        (
            Peer::with_output(1, 2, 1, true, true, tx.clone(), true)?,
            Peer::with_output(1, 1, 1, false, true, tx.clone(), true)?,
        )
    } else {
        let p = pipeline(
            "appsrc name=v is-live=true format=time min-latency=0 max-latency=-1 max-buffers=2 max-bytes=0 leaky-type=downstream ! video/x-h264,stream-format=byte-stream,alignment=au ! rtph264pay name=video_rtp config-interval=-1 pt=96 appsrc name=a is-live=true format=time min-latency=0 max-latency=-1 max-buffers=4 max-bytes=0 leaky-type=downstream ! audio/x-opus,rate=48000,channels=2,channel-mapping-family=0 ! rtpopuspay name=audio_rtp pt=97",
        )?;
        for (media, payloader) in [("video", "video_rtp"), ("audio", "audio_rtp")] {
            let bin = receive_bin(media, true)?;
            p.add(&bin).map_err(|e| e.to_string())?;
            p.get_by_name(payloader)
                .ok_or("RTP ausente")?
                .link(&bin)
                .map_err(|e| e.to_string())?;
        }
        let receiver = Peer {
            video: p
                .get_by_name("v")
                .and_then(|e| e.downcast::<gst_app::AppSrc>().ok()),
            audio: p
                .get_by_name("a")
                .and_then(|e| e.downcast::<gst_app::AppSrc>().ok()),
            rtc: gst::ElementFactory::make("webrtcbin", None).map_err(|e| e.to_string())?,
            pipeline: p,
            generation: 1,
            view: None,
            volume: Arc::new(Mutex::new(100)),
            video_frames: Arc::default(),
            audio_frames: Arc::default(),
            audio_level: Arc::default(),
            session: None,
            remote_ready: false,
            loss: Arc::default(),
            jitter: Mutex::default(),
            stats_pending: Arc::default(),
            pending: vec![],
            created: std::time::Instant::now(),
        };
        for (name, count) in [
            ("video_meter", receiver.video_frames.clone()),
            ("audio_meter", receiver.audio_frames.clone()),
        ] {
            receiver
                .pipeline
                .get_by_name(name)
                .ok_or("Medidor ausente")?
                .connect("handoff", false, move |_| {
                    count.fetch_add(1, Ordering::Relaxed);
                    None
                })
                .map_err(|e| e.to_string())?;
        }
        watch_audio(
            &receiver
                .pipeline
                .get_by_name("audio_meter")
                .unwrap()
                .get_static_pad("src")
                .unwrap(),
            receiver.audio_level.clone(),
        );
        receiver
            .pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| e.to_string())?;
        capture.add(1, &receiver);
        // A sender is only needed by the ICE branch below.
        (
            Peer::with_output(1, 2, 1, true, false, tx.clone(), true)?,
            receiver,
        )
    };
    let injected = Arc::new(AtomicU64::new(0));
    let last_loss = Arc::new(Mutex::new(None::<std::time::Instant>));
    let presentation = Arc::new(Mutex::new(Vec::new()));
    let audio_presentation = Arc::new(Mutex::new(Vec::new()));
    let audio_signal = Arc::new(Mutex::new((0i16, 0u64, 0u64)));
    if packet_loss {
        let rtp = receiver
            .pipeline
            .iterate_recurse()
            .into_iter()
            .flatten()
            .find(|element| {
                element
                    .get_factory()
                    .is_some_and(|factory| factory.get_name() == "rtpbin")
            })
            .ok_or("Receptor RTP ausente")?;
        let dropped = injected.clone();
        let decoded = receiver.video_frames.clone();
        let last_drop = last_loss.clone();
        rtp.connect_pad_added(move |_, pad| {
            if !pad.get_name().starts_with("recv_rtp_sink_") {
                return;
            }
            let (dropped, decoded) = (dropped.clone(), decoded.clone());
            let last_drop = last_drop.clone();
            pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                let Some(gst::PadProbeData::Buffer(buffer)) = &info.data else {
                    return gst::PadProbeReturn::Ok;
                };
                let Ok(bytes) = buffer.map_readable() else {
                    return gst::PadProbeReturn::Ok;
                };
                let bytes = bytes.as_slice();
                if bytes.len() >= 12
                    && bytes[1] & 127 == 96
                    && decoded.load(Ordering::Relaxed) >= 100
                    && dropped.load(Ordering::Relaxed) < 12
                {
                    let mut previous = last_drop.lock().unwrap();
                    let now = std::time::Instant::now();
                    if previous.is_none_or(|previous| {
                        now.duration_since(previous) >= Duration::from_secs(3)
                    }) {
                        *previous = Some(now);
                        dropped.fetch_add(1, Ordering::Relaxed);
                        return gst::PadProbeReturn::Drop;
                    }
                }
                gst::PadProbeReturn::Ok
            });
        });
        let times = presentation.clone();
        let audio_times = audio_presentation.clone();
        let signal = audio_signal.clone();
        receiver
            .pipeline
            .connect("deep-element-added", false, move |values| {
                if let Ok(Some(element)) = values[2].get::<gst::Element>()
                    && matches!(element.get_name().as_str(), "video_meter" | "audio_output")
                {
                    let times = if element.get_name() == "audio_output" {
                        audio_times.clone()
                    } else {
                        times.clone()
                    };
                    let audio = element.get_name() == "audio_output";
                    let signal = signal.clone();
                    element
                        .connect("handoff", false, move |values| {
                            if audio
                                && let Ok(Some(buffer)) = values[1].get::<gst::Buffer>()
                                && let Ok(bytes) = buffer.map_readable()
                            {
                                let mut signal = signal.lock().unwrap();
                                for frame in bytes.as_slice().as_chunks::<4>().0 {
                                    let sample = i16::from_ne_bytes([frame[0], frame[1]]);
                                    if signal.0 < 0 && sample >= 0 {
                                        signal.2 += 1;
                                    }
                                    signal.0 = sample;
                                    signal.1 += 1;
                                }
                            }
                            times.lock().unwrap().push(std::time::Instant::now());
                            None
                        })
                        .unwrap();
                }
                None
            })
            .map_err(|e| e.to_string())?;
    }
    if network {
        capture.add(2, &sender);
        let commands = tx.clone();
        let generation = sender.generation;
        super::rtc::create_description(&sender.rtc, true, move |r| {
            let _ = commands.try_send(match r {
                Ok(data) => Command::Signal(1, 2, 1, generation, data),
                Err(e) => Command::MediaError(1, 2, 1, generation, e),
            });
        })?;
    }
    let rendered = |p: &gst::Pipeline| {
        p.get_by_name("display")
            .and_then(|sink| sink.get_property("stats").ok())
            .and_then(|value| value.get::<gst::Structure>().ok().flatten())
            .and_then(|stats| stats.get::<u64>("rendered").ok().flatten())
            .unwrap_or(0)
    };
    let deadline =
        std::time::Instant::now() + Duration::from_secs(if packet_loss { 90 } else { 25 });
    let mut measured = None;
    let mut reconfigured = false;
    let mut latency_tick = std::time::Instant::now();
    while std::time::Instant::now() < deadline {
        if let Some(error) = capture
            .error()
            .or_else(|| pipe_error(&receiver.pipeline))
            .or_else(|| pipe_error(&sender.pipeline))
        {
            return Err(error);
        }
        if latency_tick.elapsed() >= Duration::from_secs(2) {
            receiver.tune_latency(latency_tick.elapsed());
            latency_tick = std::time::Instant::now();
        }
        if retune && !reconfigured && rendered(&receiver.pipeline) >= 30 {
            capture.retune(Settings {
                height: 720,
                fps: 15,
                bitrate: 2000,
                ..settings.clone()
            })?;
            reconfigured = true;
        }
        if rendered(&receiver.pipeline) >= 90 && receiver.audio_frames.load(Ordering::Relaxed) >= 90
        {
            let sample = measured.get_or_insert_with(|| {
                (
                    std::time::Instant::now(),
                    rendered(&receiver.pipeline),
                    capture.frames.load(Ordering::Relaxed),
                    capture.raw_frames.load(Ordering::Relaxed),
                    capture.encoder_input.load(Ordering::Relaxed),
                )
            });
            if sample.0.elapsed() >= Duration::from_secs(if packet_loss { 60 } else { 3 }) {
                break;
            }
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Command::Signal(_, to, _, _, data)) if network => {
                let peer = if to == 2 { &mut receiver } else { &mut sender };
                if let Some(d) = data.get("description") {
                    let kind = d["type"].as_str().ok_or("Tipo SDP ausente")?;
                    let offer = kind == "offer";
                    let commands = tx.clone();
                    let generation = peer.generation;
                    super::rtc::set_remote(
                        &peer.rtc,
                        kind,
                        d["sdp"].as_str().ok_or("SDP ausente")?,
                        move |r| {
                            let _ = commands
                                .try_send(Command::RemoteReady(1, to, 1, generation, offer, r));
                        },
                    )?;
                } else if let Some(c) = data.get("candidate") {
                    peer.candidate(
                        c["sdpMLineIndex"].as_u64().ok_or("ICE ausente")? as u32,
                        c["candidate"].as_str().ok_or("ICE ausente")?.into(),
                        None,
                    )?;
                }
            }
            Ok(Command::RemoteReady(_, to, _, _, offer, result)) if network => {
                result?;
                let peer = if to == 2 { &mut receiver } else { &mut sender };
                peer.ready()?;
                if offer {
                    let commands = tx.clone();
                    let generation = receiver.generation;
                    super::rtc::create_description(&receiver.rtc, false, move |r| {
                        let _ = commands.try_send(match r {
                            Ok(data) => Command::Signal(1, 1, 1, generation, data),
                            Err(e) => Command::MediaError(1, 1, 1, generation, e),
                        });
                    })?;
                }
            }
            Ok(Command::MediaError(_, _, _, _, error)) => return Err(error),
            _ => {}
        }
    }
    let mut loss_report = serde_json::Value::Null;
    if packet_loss {
        let times = presentation.lock().unwrap();
        let maximum = times
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]))
            .chain(times.last().map(|last| last.elapsed()))
            .max()
            .unwrap_or(Duration::MAX);
        println!(
            "RTP loss test: {} packets dropped, {} frames, maximum video gap {} ms, {} audio buffers",
            injected.load(Ordering::Relaxed),
            times.len(),
            maximum.as_millis(),
            receiver.audio_frames.load(Ordering::Relaxed)
        );
        let audio_times = audio_presentation.lock().unwrap();
        let audio_maximum = audio_times
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]))
            .chain(audio_times.last().map(|last| last.elapsed()))
            .max()
            .unwrap_or(Duration::MAX);
        let steady_start = last_loss
            .lock()
            .unwrap()
            .ok_or("Nenhuma perda foi injetada")?
            + Duration::from_secs(5);
        let steady_gap = times
            .windows(2)
            .filter(|pair| pair[0] >= steady_start)
            .map(|pair| pair[1].duration_since(pair[0]))
            .max()
            .unwrap_or(Duration::MAX);
        let steady_fps = times.iter().filter(|time| **time >= steady_start).count() as f64
            / steady_start.elapsed().as_secs_f64();
        if steady_gap > Duration::from_millis(150) || steady_fps < 24.0 {
            return Err(format!(
                "Vídeo sem fluidez após recuperar a rede: {} ms / {steady_fps:.2} FPS",
                steady_gap.as_millis()
            ));
        }
        let signal = audio_signal.lock().unwrap();
        let frequency = signal.2 as f64 * 48_000.0 / signal.1.max(1) as f64;
        loss_report = json!({"dropped_video_packets":injected.load(Ordering::Relaxed),"maximum_video_gap_ms":maximum.as_millis(),"maximum_audio_gap_ms":audio_maximum.as_millis(),"audio_test_frequency_hz":frequency,"steady_max_video_gap_ms":steady_gap.as_millis(),"steady_video_fps":steady_fps});
        if !(430.0..=450.0).contains(&frequency) {
            return Err(format!(
                "O áudio recebido não corresponde ao tom de teste de 440 Hz: {loss_report}"
            ));
        }
        if audio_maximum > Duration::from_millis(100) {
            return Err(format!("Áudio interrompido: {loss_report}"));
        }
        if injected.load(Ordering::Relaxed) != 12 || maximum > Duration::from_millis(500) {
            return Err(format!(
                "Reprodução interrompida após perda RTP: {} pacotes, maior intervalo {} ms",
                injected.load(Ordering::Relaxed),
                maximum.as_millis()
            ));
        }
    }
    let video_frames = rendered(&receiver.pipeline);
    let audio_frames = receiver.audio_frames.load(Ordering::Relaxed);
    let audio_peak = receiver.audio_level.peak();
    if audio_peak < 0.01 {
        return Err(format!(
            "Autoteste recebeu áudio sem o sinal sonoro esperado (pico {audio_peak:.4})"
        ));
    }
    if video_frames < 90 || audio_frames < 90 {
        return Err(format!(
            "Autoteste expirou: {video_frames} quadros, {audio_frames} blocos de áudio; rede={network}"
        ));
    }
    let caps = receiver
        .pipeline
        .get_by_name("video_meter")
        .and_then(|e| e.get_static_pad("src"))
        .and_then(|p| p.get_current_caps())
        .ok_or("Resolução recebida ausente")?;
    let structure = caps.get_structure(0).ok_or("Formato recebido ausente")?;
    let width = structure
        .get::<i32>("width")
        .map_err(|e| e.to_string())?
        .ok_or("Largura ausente")?;
    let height = structure
        .get::<i32>("height")
        .map_err(|e| e.to_string())?
        .ok_or("Altura ausente")?;
    let expected = if retune { (1280, 720) } else { (1920, 1080) };
    if (width, height) != expected {
        return Err(format!("Resolução incorreta: {width}x{height}"));
    }
    if capture.pipeline.get_by_name("preview").is_some() {
        return Err("Captura ainda está gerando prévia".into());
    }
    let display = receiver
        .pipeline
        .get_by_name("display")
        .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
        .and_then(|s| s.try_pull_sample(gst::ClockTime::from_seconds(1)))
        .ok_or("Receptor não entregou imagem ao app")?;
    let displayed = super::video::Frame::new(&display).ok_or("Imagem nativa inválida")?;
    if (displayed.width(), displayed.height()) != expected {
        return Err("Resolução da imagem do app incorreta".into());
    }
    let sample = measured.ok_or("Não houve quadros suficientes para medir FPS")?;
    let seconds = sample.0.elapsed().as_secs_f64();
    let received_fps = (video_frames - sample.1) as f64 / seconds;
    let encoded_fps = (capture.frames.load(Ordering::Relaxed) - sample.2) as f64 / seconds;
    let raw_fps = (capture.raw_frames.load(Ordering::Relaxed) - sample.3) as f64 / seconds;
    let encoder_input_fps =
        (capture.encoder_input.load(Ordering::Relaxed) - sample.4) as f64 / seconds;
    let latency = |pipeline: &gst::Pipeline| {
        let mut query = gst::query::Latency::new();
        pipeline
            .query(&mut query)
            .then(|| query.get_result().1.0.map(|ns| ns as f64 / 1_000_000.0))
            .flatten()
    };
    let report = json!({"loss_recovery":loss_report,"capture_min_latency_ms":latency(&capture.pipeline),"receiver_min_latency_ms":latency(&receiver.pipeline),"preview":false, "display":format!("{width}x{height} {}",if cfg!(target_endian="little"){"BGRx"}else{"xRGB"}), "received_fps":received_fps,"encoded_fps":encoded_fps,"raw_fps":raw_fps,"encoder_input_fps":encoder_input_fps,"measurement_seconds":seconds,"test":if network {"webrtc-loopback"} else {"encoder-bridge"},"encoder":capture.encoder,"width":width,"height":height,"video_frames":video_frames,"audio_buffers":audio_frames,"audio_peak":audio_peak,"seconds_including_negotiation":start.elapsed().as_secs_f64()});
    super::diagnostics::event!("self-test", report.clone());
    // Stop the shared producer before tearing down the receiving decoder.
    capture
        .pipeline
        .set_state(gst::State::Null)
        .map_err(|e| e.to_string())?;
    if received_fps < if retune { 12.0 } else { 24.0 } {
        return Err(format!(
            "Desempenho abaixo de 24 FPS no teste 1080p30: {report}"
        ));
    }
    Ok(report)
}

#[cfg(any(test, feature = "diagnostics"))]
pub fn check_runtime() -> Result<(), String> {
    gst::init().map_err(|e| e.to_string())?;
    if gst::version().0 == 1 && gst::version().1 < 22 {
        return Err("É necessário GStreamer 1.22 ou mais recente".into());
    }
    for name in [
        "webrtcbin",
        "nicesrc",
        "dtlsenc",
        "srtpenc",
        "appsrc",
        "appsink",
        "openh264enc",
        "openh264dec",
        "avdec_h264",
        "h264parse",
        "opusenc",
        "opusdec",
        "rtph264pay",
        "rtph264depay",
        "rtpopuspay",
        "rtpopusdepay",
        "autoaudiosink",
        "videotestsrc",
        "audiotestsrc",
        "volume",
    ] {
        if gst::ElementFactory::find(name).is_none() {
            return Err(format!(
                "Plugin ausente: {name}. Gere novamente o pacote nativo."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_level_verifies_real_decoded_sound_silence_and_mute() {
        gst::init().unwrap();
        for (wave, gain, expected) in [
            ("sine", 1.0, "sinal"),
            ("silence", 1.0, "silêncio"),
            ("sine", 0.0, "silêncio"),
        ] {
            let chain = receive_audio("fakesink sync=false");
            let p = pipeline(&format!("audiotestsrc num-buffers=20 samplesperbuffer=960 wave={wave} volume=0.1 ! audioconvert ! audio/x-raw,rate=48000,channels=2 ! {AUDIO_ENCODER} ! rtpopuspay ! {chain}")).unwrap();
            p.get_by_name("playback_volume")
                .unwrap()
                .set_property("volume", &gain)
                .unwrap();
            let level = Arc::new(AudioLevel::default());
            assert_eq!(level.status(), "sem dados");
            watch_audio(
                &p.get_by_name("audio_meter")
                    .unwrap()
                    .get_static_pad("src")
                    .unwrap(),
                level.clone(),
            );
            p.set_state(gst::State::Playing).unwrap();
            let done = p.get_bus().unwrap().timed_pop_filtered(
                gst::ClockTime::from_seconds(5),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            );
            p.set_state(gst::State::Null).unwrap();
            assert!(
                matches!(
                    done.as_ref().map(|m| m.view()),
                    Some(gst::MessageView::Eos(_))
                ),
                "{done:?}"
            );
            if expected == "sinal" {
                assert!(level.peak() > 0.03);
            }
            assert_eq!(level.status(), expected, "wave={wave}, gain={gain}");
            assert_eq!(
                level.status(),
                "sem dados",
                "Stale audio must not look active"
            );
        }
        let level = AudioLevel::default();
        level.observe(gst::Buffer::new().as_ref());
        assert_eq!(level.status(), "sem dados");
        level.observe(gst::Buffer::from_slice(i16::MIN.to_ne_bytes().to_vec()).as_ref());
        assert_eq!(level.peak(), 1.0);
    }
    #[test]
    fn windows_loopback_uses_process_capture_when_supported_and_preserves_legacy() {
        let process = windows_loopback(true, None).unwrap();
        assert!(process.contains("loopback-mode=exclude-process-tree"));
        assert!(process.contains(&format!("loopback-target-pid={}", std::process::id())));
        assert!(
            windows_loopback(true, Some(1234))
                .unwrap()
                .contains("loopback-target-pid=1234")
        );
        assert!(windows_loopback(true, Some(0)).is_err());
        assert!(windows_loopback(false, Some(1234)).is_err());
        assert_eq!(
            windows_loopback(false, None).unwrap(),
            "wasapi2src loopback=true low-latency=true"
        );
    }
    #[test]
    fn opus_fec_advertises_one_real_packet_of_latency() {
        gst::init().unwrap();
        let source = pipeline(&format!("audiotestsrc is-live=true num-buffers=30 samplesperbuffer=480 ! audio/x-raw,rate=48000,channels=2 ! {AUDIO_ENCODER} ! rtpopuspay name=rtp")).unwrap();
        let bin = receive_bin("audio", true).unwrap();
        source.add(&bin).unwrap();
        source.get_by_name("rtp").unwrap().link(&bin).unwrap();
        source.set_state(gst::State::Playing).unwrap();
        let message = source.get_bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(3),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        let decoder = bin.get_by_name("audio_decoder").unwrap();
        let mut upstream = gst::query::Latency::new();
        let mut decoded = gst::query::Latency::new();
        assert!(
            decoder
                .get_static_pad("sink")
                .unwrap()
                .peer_query(&mut upstream)
        );
        assert!(decoder.query(&mut decoded));
        source.set_state(gst::State::Null).unwrap();
        assert!(matches!(
            message.as_ref().map(|m| m.view()),
            Some(gst::MessageView::Eos(_))
        ));
        assert_eq!(
            decoded.get_result().1 - upstream.get_result().1,
            gst::ClockTime::from_mseconds(10)
        );
        assert_eq!(
            decoder
                .get_property("use-inband-fec")
                .unwrap()
                .get::<bool>()
                .unwrap(),
            Some(true)
        );
    }

    #[test]
    fn audio_jitter_tolerates_reordering_and_conceals_one_lost_packet() {
        audio_jitter_case(false);
    }
    #[test]
    fn audio_jitter_recovers_when_loss_notice_is_evicted() {
        audio_jitter_case(true);
    }
    fn audio_jitter_case(discard_loss_notice: bool) {
        gst::init().unwrap();
        let source = pipeline(&format!("audiotestsrc num-buffers=60 samplesperbuffer=480 ! audioconvert ! audio/x-raw,rate=48000,channels=2 ! {AUDIO_ENCODER} ! rtpopuspay ! appsink name=packets sync=false")).unwrap();
        let packets = source
            .get_by_name("packets")
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap();
        source.set_state(gst::State::Playing).unwrap();
        let mut samples = Vec::new();
        while let Some(sample) = packets.try_pull_sample(gst::ClockTime::from_seconds(2)) {
            samples.push(sample);
        }
        source.set_state(gst::State::Null).unwrap();
        assert!(samples.len() >= 25);
        let (tx, _rx) = std::sync::mpsc::sync_channel(32);
        let peer = Peer::with_output(1, 2, 1, false, true, tx, true).unwrap();
        assert_eq!(
            peer.rtc
                .get_property("latency")
                .unwrap()
                .get::<u32>()
                .unwrap(),
            Some(MIN_JITTER_MS)
        );
        peer.rtc.set_property("latency", &150u32).unwrap();
        let latency = peer
            .rtc
            .get_property("latency")
            .unwrap()
            .get::<u32>()
            .unwrap()
            .unwrap();
        drop(peer);
        let p = pipeline(&format!("appsrc name=network is-live=true format=time ! rtpjitterbuffer name=jitter latency={latency} do-lost=true")).unwrap();
        let audio = receive_bin("audio", true).unwrap();
        p.add(&audio).unwrap();
        p.get_by_name("jitter").unwrap().link(&audio).unwrap();
        if discard_loss_notice {
            audio
                .get_by_name("audio_depay")
                .unwrap()
                .get_static_pad("src")
                .unwrap()
                .add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, |_, info| {
                    if let Some(gst::PadProbeData::Event(event)) = &info.data
                        && matches!(event.view(), gst::EventView::Gap(_))
                    {
                        return gst::PadProbeReturn::Drop;
                    }
                    gst::PadProbeReturn::Ok
                });
        }
        let app = p
            .get_by_name("network")
            .unwrap()
            .downcast::<gst_app::AppSrc>()
            .unwrap();
        configure_receiver_element(&p.get_by_name("jitter").unwrap());
        app.set_caps(samples[0].get_caps().map(|c| c.to_owned()).as_ref());
        p.set_state(gst::State::Playing).unwrap();
        let start = std::time::Instant::now();
        let delayed = samples.len() / 3;
        let late_time = samples[delayed].get_buffer().unwrap().get_pts().0.unwrap() + 90_000_000;
        let mut sent_delayed = false;
        let push = |sample: &gst::Sample| {
            let mut buffer = sample.get_buffer().unwrap().to_owned();
            let data = buffer.make_mut();
            data.set_pts(p.get_current_running_time());
            // RTP arrival time must not retain the encoder's old DTS during replay/reordering.
            data.set_dts(gst::CLOCK_TIME_NONE);
            app.push_buffer(buffer).unwrap();
        };
        for (index, sample) in samples.iter().enumerate() {
            if index == samples.len() * 2 / 3 || index == delayed {
                continue;
            }
            let pts = sample.get_buffer().unwrap().get_pts().0.unwrap();
            let target = start + Duration::from_nanos(pts);
            if let Some(wait) = target.checked_duration_since(std::time::Instant::now()) {
                std::thread::sleep(wait);
            }
            if !sent_delayed && pts >= late_time {
                push(&samples[delayed]);
                sent_delayed = true;
            }
            push(sample);
        }
        app.end_of_stream().unwrap();
        let done = p.get_bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(3),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        let stats = p
            .get_by_name("jitter")
            .unwrap()
            .get_property("stats")
            .unwrap()
            .get::<gst::Structure>()
            .unwrap()
            .unwrap();
        let decoded = p
            .get_by_name("audio_decoder")
            .unwrap()
            .get_property("stats")
            .unwrap()
            .get::<gst::Structure>()
            .unwrap()
            .unwrap();
        p.set_state(gst::State::Null).unwrap();
        assert!(
            matches!(
                done.as_ref().map(|m| m.view()),
                Some(gst::MessageView::Eos(_))
            ),
            "{done:?}"
        );
        assert!(sent_delayed);
        assert_eq!(
            stats.get::<u64>("num-lost").unwrap(),
            Some(1),
            "A packet delayed by 90 ms caused extra audio loss: {stats}"
        );
        assert!(
            (360..=600).contains(&decoded.get::<u64>("plc-num-samples").unwrap().unwrap()),
            "One 10 ms loss must be concealed once, including timestamp rounding: {decoded}"
        );
    }
    #[test]
    fn calibration_preserves_audio_bitrate_and_never_increases_requested_quality() {
        gst::init().unwrap();
        let requested = Settings {
            height: 720,
            fps: 15,
            bitrate: 6000,
            audio: true,
            source: 1234,
            ..Settings::default()
        };
        let (profile, report) = recommend(&requested).unwrap();
        assert_eq!(
            (
                profile.height,
                profile.fps,
                profile.bitrate,
                profile.audio,
                profile.source
            ),
            (720, 15, 6000, true, 0)
        );
        assert!(!report.is_empty());
    }
    #[test]
    fn codec_budget_leaves_room_for_the_desktop() {
        for (cores, expected) in [(0, 1), (1, 1), (2, 1), (4, 3), (8, 4), (64, 4)] {
            assert_eq!(thread_budget(cores), expected);
        }
        gst::init().unwrap();
        for encoder in encoders(&Settings::default()) {
            if encoder.starts_with("x264enc") {
                assert!(encoder.contains("threads="));
            }
            if encoder.starts_with("openh264enc") {
                assert!(encoder.contains("multi-thread="));
            }
        }
    }
    #[test]
    fn receiver_audio_follows_external_clock_with_resampling() {
        gst::init().unwrap();
        for name in ["pulsesink", "wasapisink", "wasapi2sink"] {
            if let Ok(element) = gst::ElementFactory::make(name, None) {
                configure_receiver_element(&element);
                let method = element.get_property("slave-method").unwrap();
                assert_eq!(
                    glib_old::EnumValue::from_value(&method).unwrap().get_nick(),
                    "resample"
                );
                assert_eq!(
                    element
                        .get_property("provide-clock")
                        .unwrap()
                        .get::<bool>()
                        .unwrap(),
                    Some(false)
                );
                assert_eq!(
                    element
                        .get_property("buffer-time")
                        .unwrap()
                        .get::<i64>()
                        .unwrap(),
                    Some(40_000),
                    "Playback should not retain the default 200 ms buffer"
                );
                assert_eq!(
                    element
                        .get_property("latency-time")
                        .unwrap()
                        .get::<i64>()
                        .unwrap(),
                    Some(10_000)
                );
            }
        }
    }
    #[test]
    fn receiver_does_not_decode_dependent_frames_after_rtp_loss() {
        gst::init().unwrap();
        let chain = receive_video("fakesink sync=false async=false");
        let p = pipeline(&format!("videotestsrc num-buffers=25 ! video/x-raw,format=I420,width=320,height=180,framerate=30/1 ! openh264enc gop-size=10 ! h264parse ! rtph264pay name=pay pt=96 config-interval=-1 ! {chain}")).unwrap();
        let lost = Arc::new(AtomicBool::new(false));
        let dropped = lost.clone();
        p.get_by_name("pay")
            .unwrap()
            .get_static_pad("src")
            .unwrap()
            .add_probe(
                gst::PadProbeType::BUFFER | gst::PadProbeType::BUFFER_LIST,
                move |_, info| {
                    let pts = match &info.data {
                        Some(gst::PadProbeData::Buffer(b)) => b.get_pts().0,
                        Some(gst::PadProbeData::BufferList(list)) => {
                            list.get(0).and_then(|b| b.get_pts().0)
                        }
                        _ => None,
                    };
                    if pts.is_some_and(|n| (166_000_000..200_000_000).contains(&n)) {
                        dropped.store(true, Ordering::Relaxed);
                        gst::PadProbeReturn::Drop
                    } else {
                        gst::PadProbeReturn::Ok
                    }
                },
            );
        let seen = Arc::new(Mutex::new(Vec::new()));
        let output = seen.clone();
        p.get_by_name("video_depay")
            .unwrap()
            .get_static_pad("src")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                if let Some(gst::PadProbeData::Buffer(b)) = &info.data {
                    output.lock().unwrap().push((
                        b.get_pts().0.unwrap(),
                        b.get_flags().contains(gst::BufferFlags::DELTA_UNIT),
                    ));
                }
                gst::PadProbeReturn::Ok
            });
        p.set_state(gst::State::Playing).unwrap();
        let done = p.get_bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(5),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        p.set_state(gst::State::Null).unwrap();
        assert!(
            matches!(
                done.as_ref().map(|m| m.view()),
                Some(gst::MessageView::Eos(_))
            ),
            "{done:?}"
        );
        assert!(lost.load(Ordering::Relaxed), "Loss injector did not run");
        let pts = seen.lock().unwrap();
        let recovered = pts
            .iter()
            .find(|(n, _)| *n >= 200_000_000)
            .expect("No recovery");
        assert!(
            !recovered.1,
            "Decoded dependent frame after packet loss: {pts:?}"
        );
    }
    #[test]
    fn slow_video_output_keeps_recent_frames_with_qos() {
        gst::init().unwrap();
        for decoder in ["decodebin", "openh264dec"] {
            let chain = receive_video(&format!("identity sleep-time=80000 ! {DISPLAY_SINK}"));
            let chain = chain.replace("! decodebin !", &format!("! {decoder} !"));
            let p = pipeline(&format!("videotestsrc is-live=true ! video/x-raw,format=I420,width=320,height=180,framerate=30/1 ! openh264enc gop-size=30 ! h264parse ! rtph264pay pt=96 ! {chain}")).unwrap();
            let sink = p
                .get_by_name("display")
                .unwrap()
                .downcast::<gst_app::AppSink>()
                .unwrap();
            p.set_state(gst::State::Playing).unwrap();
            let mut age = 0;
            for _ in 0..15 {
                let sample = sink
                    .try_pull_sample(gst::ClockTime::from_seconds(2))
                    .expect("Video stopped");
                let now =
                    p.get_clock().unwrap().get_time().0.unwrap() - p.get_base_time().0.unwrap();
                age = now.saturating_sub(sample.get_buffer().unwrap().get_pts().0.unwrap());
            }
            p.set_state(gst::State::Null).unwrap();
            println!(
                "Slow video output ({decoder}): {} ms frame age",
                age / 1_000_000
            );
            assert!(
                age < 250_000_000,
                "Slow output accumulated {} ms of stale video",
                age / 1_000_000
            );
        }
    }
    #[test]
    fn encoded_queue_overflow_waits_for_a_complete_keyframe() {
        gst::init().unwrap();
        let p = pipeline("appsrc name=v is-live=true format=time max-buffers=2 max-bytes=0 leaky-type=downstream ! appsink name=out sync=false async=false").unwrap();
        let app = p
            .get_by_name("v")
            .unwrap()
            .downcast::<gst_app::AppSrc>()
            .unwrap();
        let out = p
            .get_by_name("out")
            .unwrap()
            .downcast::<gst_app::AppSink>()
            .unwrap();
        protect_h264_references(&app);
        let push = |delta: bool, index: u64| {
            let mut b = gst::Buffer::with_size(1).unwrap();
            let bmut = b.get_mut().unwrap();
            bmut.set_pts(gst::ClockTime::from_mseconds(index * 33));
            if delta {
                bmut.set_flags(gst::BufferFlags::DELTA_UNIT);
            }
            app.push_buffer(b).unwrap();
        };
        p.set_state(gst::State::Paused).unwrap();
        // Exactly the production overflow: IDR is evicted, leaving two dependent frames.
        push(false, 0);
        push(true, 1);
        push(true, 2);
        p.set_state(gst::State::Playing).unwrap();
        let damaged = out.try_pull_sample(gst::ClockTime::from_mseconds(200));
        push(false, 3);
        let recovered = out.try_pull_sample(gst::ClockTime::from_seconds(1));
        push(true, 4);
        let next = out.try_pull_sample(gst::ClockTime::from_seconds(1));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        app.get_static_pad("src")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                if let Some(gst::PadProbeData::Buffer(b)) = &info.data
                    && b.get_pts().0 == Some(5 * 33_000_000)
                {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap();
                }
                gst::PadProbeReturn::Ok
            });
        push(true, 5);
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for index in 6..10 {
            push(true, index);
        }
        release_tx.send(()).unwrap();
        let mut midstream = Vec::new();
        while let Some(sample) = out.try_pull_sample(gst::ClockTime::from_mseconds(200)) {
            midstream.push(sample.get_buffer().unwrap().get_pts().0.unwrap());
        }
        push(false, 10);
        let recovered_midstream = out.try_pull_sample(gst::ClockTime::from_seconds(1));
        p.set_state(gst::State::Null).unwrap();
        assert!(
            midstream.iter().all(|pts| *pts == 5 * 33_000_000),
            "Mid-stream overflow forwarded broken references: {midstream:?}"
        );
        assert!(
            !recovered_midstream
                .unwrap()
                .get_buffer()
                .unwrap()
                .get_flags()
                .contains(gst::BufferFlags::DELTA_UNIT)
        );
        assert!(
            damaged.is_none(),
            "Dependent H264 frame escaped after its reference was dropped"
        );
        assert!(
            !recovered
                .unwrap()
                .get_buffer()
                .unwrap()
                .get_flags()
                .contains(gst::BufferFlags::DELTA_UNIT)
        );
        assert!(
            next.unwrap()
                .get_buffer()
                .unwrap()
                .get_flags()
                .contains(gst::BufferFlags::DELTA_UNIT)
        );
    }
    #[test]
    fn latency_bus_message_is_distributed_to_sources() {
        gst::init().unwrap();
        let p = pipeline("fakesrc is-live=true ! identity name=clock_probe ! fakesink sync=false")
            .unwrap();
        let count = Arc::new(AtomicU64::new(0));
        let seen = count.clone();
        p.get_by_name("clock_probe")
            .unwrap()
            .get_static_pad("src")
            .unwrap()
            .add_probe(gst::PadProbeType::EVENT_UPSTREAM, move |_, info| {
                if let Some(gst::PadProbeData::Event(e)) = &info.data
                    && matches!(e.view(), gst::EventView::Latency(_))
                {
                    seen.fetch_add(1, Ordering::Relaxed);
                }
                gst::PadProbeReturn::Ok
            });
        p.set_state(gst::State::Playing).unwrap();
        let _ = p.get_state(gst::ClockTime::from_seconds(1));
        count.store(0, Ordering::Relaxed);
        p.get_bus()
            .unwrap()
            .post(&gst::message::Latency::builder().src(&p).build())
            .unwrap();
        let error = pipe_error(&p);
        let distributed = count.load(Ordering::Relaxed);
        p.set_state(gst::State::Null).unwrap();
        assert!(error.is_none());
        assert!(distributed > 0, "LATENCY message was discarded");
    }
    #[test]
    fn dynamic_receiver_exposes_rtp_input_not_raw_decoder_input() {
        gst::init().unwrap();
        let bin = receive_bin("video", true).unwrap();
        let sink = bin.get_static_pad("sink").unwrap();
        let rtp: gst::Caps =
            "application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000"
                .parse()
                .unwrap();
        assert!(
            sink.query_caps(None).unwrap().can_intersect(&rtp),
            "Receiver ghost pad does not accept RTP: {:?}",
            sink.query_caps(None)
        );
    }
    #[test]
    fn receiver_negotiates_rgb_display_from_h264_i420() {
        gst::init().unwrap();
        let chain = receive_video("video/x-raw,format=RGB ! fakesink sync=false");
        let p = pipeline(&format!("videotestsrc num-buffers=12 ! video/x-raw,format=I420,width=320,height=180,framerate=30/1 ! openh264enc complexity=low ! rtph264pay pt=96 config-interval=-1 ! {chain}")).unwrap();
        let frames = Arc::new(AtomicU64::new(0));
        let count = frames.clone();
        p.get_by_name("video_meter")
            .unwrap()
            .connect("handoff", false, move |_| {
                count.fetch_add(1, Ordering::Relaxed);
                None
            })
            .unwrap();
        p.set_state(gst::State::Playing).unwrap();
        let message = p.get_bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(8),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        p.set_state(gst::State::Null).unwrap();
        match message.as_ref().map(|m| m.view()) {
            Some(gst::MessageView::Eos(_)) => assert!(
                frames.load(Ordering::Relaxed) > 0,
                "No decoded frames reached display"
            ),
            Some(gst::MessageView::Error(e)) => panic!("{} {:?}", e.get_error(), e.get_debug()),
            _ => panic!("Receiver stalled during negotiation"),
        }
    }
    #[test]
    fn adaptive_reconfiguration_keeps_video_audio_and_session_alive() {
        let report = test_stream(false, true).unwrap();
        assert_eq!(report["width"], 1280);
        assert_eq!(report["height"], 720);
        assert!(report["received_fps"].as_f64().unwrap() >= 12.0);
        assert!(report["received_fps"].as_f64().unwrap() < 18.0);
        assert!(report["audio_buffers"].as_u64().unwrap() >= 90);
    }
    #[test]
    fn shared_encoder_bridge_delivers_full_resolution_video_and_audio() {
        let report = self_test(false).unwrap();
        assert_eq!(report["width"], 1920);
        assert!(report["video_frames"].as_u64().unwrap() >= 90);
        assert!(
            report["audio_peak"].as_f64().unwrap_or(0.0) > 0.01,
            "Decoded audio was not verified as non-silent"
        );
    }
    #[test]
    #[ignore = "requires UDP sockets; run on Windows/Linux before distributing"]
    fn actual_webrtc_loopback_decodes_video_and_audio() {
        let report = self_test(true).unwrap();
        println!("{report}");
        assert!(
            report["receiver_min_latency_ms"].as_f64().unwrap() < 150.0,
            "Unexpected latency on loopback: {report}"
        );
    }
    #[test]
    #[ignore = "requires UDP sockets; run on Windows/Linux before distributing"]
    fn actual_webrtc_loopback_adapts_video_without_losing_audio() {
        test_stream(true, true).unwrap();
    }
    #[test]
    #[ignore = "requires UDP sockets; 60 seconds of deliberate packet loss with continuous audio"]
    fn actual_webrtc_loopback_recovers_video_loss_with_continuous_audio() {
        let report = test_stream_case(true, false, true).unwrap();
        println!("{report}");
        assert!(report["received_fps"].as_f64().unwrap() > 20.0);
        assert!(report["audio_buffers"].as_u64().unwrap() > 90);
    }

    #[test]
    fn shared_encoder_uses_segment_time_and_skips_frames_before_peer_started() {
        gst::init().unwrap();
        let offset = gst::ClockTime::from_seconds(3_600_000);
        let mut segment = gst::FormattedSegment::<gst::ClockTime>::new();
        segment.set_start(offset);
        let mut buffer = gst::Buffer::new();
        buffer
            .get_mut()
            .unwrap()
            .set_pts(offset + gst::ClockTime::from_seconds(2));
        let sample = gst::Sample::builder()
            .buffer(&buffer)
            .segment(&segment)
            .build();
        assert_eq!(
            sample_time(
                &sample,
                gst::ClockTime::from_seconds(10),
                gst::ClockTime::from_seconds(11)
            ),
            gst::ClockTime::from_seconds(1)
        );
        assert_eq!(
            sample_time(
                &sample,
                gst::ClockTime::from_seconds(10),
                gst::ClockTime::from_seconds(13)
            ),
            gst::CLOCK_TIME_NONE
        );
    }
    #[test]
    fn software_video_encodes_and_decodes_without_a_display() {
        gst::init().unwrap();
        let p=pipeline("videotestsrc num-buffers=6 ! video/x-raw,width=320,height=180,framerate=30/1 ! openh264enc complexity=low bitrate=500000 ! h264parse ! openh264dec ! fakesink sync=false").unwrap();
        p.set_state(gst::State::Playing).unwrap();
        let message = p.get_bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(10),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        p.set_state(gst::State::Null).unwrap();
        match message.as_ref().map(|m| m.view()) {
            Some(gst::MessageView::Eos(_)) => {}
            Some(gst::MessageView::Error(e)) => panic!("{}", e.get_error()),
            _ => panic!("Encoder timed out"),
        }
    }
    #[test]
    fn receiver_accepts_offer_and_creates_answer() {
        gst::init().unwrap();
        let (commands, _rx) = std::sync::mpsc::sync_channel(32);
        let sender = Peer::new(1, 2, 1, true, true, commands.clone()).unwrap();
        let receiver = Peer::new(1, 1, 1, false, true, commands).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::rtc::create_description(&sender.rtc, true, move |r| {
            let _ = tx.send(r);
        })
        .unwrap();
        let offer = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::rtc::set_remote(
            &receiver.rtc,
            "offer",
            offer["description"]["sdp"].as_str().unwrap(),
            move |r| {
                let _ = tx.send(r);
            },
        )
        .unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::rtc::create_description(&receiver.rtc, false, move |r| {
            let _ = tx.send(r);
        })
        .unwrap();
        let answer = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(answer["description"]["type"], "answer");
        let sdp = answer["description"]["sdp"].as_str().unwrap();
        assert!(sdp.contains("a=rtcp-fb:96 nack\r\n"));
        assert!(sdp.contains("rtx/90000"));
        assert!(sdp.contains("OPUS/48000/2") || sdp.contains("opus/48000/2"));
        assert!(sdp.contains("useinbandfec=1"));
        assert!(
            answer["description"]["sdp"]
                .as_str()
                .unwrap()
                .contains("H264/90000")
        );
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::rtc::set_remote(
            &sender.rtc,
            "answer",
            answer["description"]["sdp"].as_str().unwrap(),
            move |r| {
                let _ = tx.send(r);
            },
        )
        .unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    }

    #[test]
    fn sender_creates_offer_with_video_and_audio() {
        gst::init().unwrap();
        let (tx, _rx) = std::sync::mpsc::sync_channel(32);
        let peer = Peer::new(1, 2, 1, true, true, tx).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::rtc::create_description(&peer.rtc, true, move |result| {
            tx.send(result).unwrap();
        })
        .unwrap();
        let value = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        let sdp = value["description"]["sdp"].as_str().unwrap();
        let mut payloads = std::collections::HashMap::new();
        for line in sdp
            .lines()
            .filter_map(|line| line.strip_prefix("a=rtpmap:"))
        {
            let (payload, codec) = line.split_once(' ').unwrap();
            if let Some(previous) = payloads.insert(payload, codec) {
                assert_eq!(
                    previous, codec,
                    "BUNDLE reuses payload {payload} for different codecs"
                );
            }
        }
        assert!(
            sdp.contains("a=ssrc:"),
            "Offer must identify RTP sources before first frame"
        );
        assert!(sdp.contains("m=video"));
        assert!(sdp.contains("H264/90000"));
        assert!(
            sdp.contains("a=rtcp-fb:96 nack\r\n"),
            "Lost video packets cannot be requested again"
        );
        assert!(
            sdp.contains("rtx/90000"),
            "Video retransmission was not negotiated"
        );
        assert!(sdp.contains("m=audio"));
        assert!(
            sdp.contains("OPUS/48000/2") || sdp.contains("opus/48000/2"),
            "Opus RTP must advertise two channels"
        );
    }
}
