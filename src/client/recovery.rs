use std::time::{Duration, Instant};

const STALL: Duration = Duration::from_secs(6);

pub(super) struct Progress {
    started: Instant,
    video: (u64, Instant),
    video_packets: u64,
    audio: (u64, Instant),
    audio_packets: u64,
    disconnected: Option<Instant>,
}
impl Progress {
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            video: (0, now),
            video_packets: 0,
            audio: (0, now),
            audio_packets: 0,
            disconnected: None,
        }
    }
    pub fn stalled(
        &mut self,
        now: Instant,
        video: u64,
        audio: u64,
        video_packets: u64,
        audio_packets: u64,
        connected: bool,
    ) -> Option<&'static str> {
        if video != self.video.0 {
            self.video = (video, now);
            self.video_packets = video_packets;
        }
        if audio != self.audio.0 {
            self.audio = (audio, now);
            self.audio_packets = audio_packets;
        }
        if connected {
            self.disconnected = None;
        } else if now.saturating_duration_since(*self.disconnected.get_or_insert(now))
            >= Duration::from_secs(10)
            && now.saturating_duration_since(self.started) >= Duration::from_secs(25)
        {
            return Some("A conexão de mídia foi interrompida");
        }
        // A static PipeWire screen may legitimately send no new frames.
        if self.video.0 > 0
            && video_packets > self.video_packets
            && now.saturating_duration_since(self.video.1) >= STALL
        {
            return Some("O vídeo parou de receber quadros");
        }
        // WASAPI and remote DTX can legitimately stop sending during silence.
        if audio_packets > self.audio_packets
            && now.saturating_duration_since(self.audio.1) >= STALL
        {
            return Some("O áudio parou de receber dados");
        }
        None
    }
}

pub(super) struct Retry {
    pub after: Option<Instant>,
    pub volume: u8,
    delay: u64,
}
impl Default for Retry {
    fn default() -> Self {
        Self {
            after: None,
            volume: 100,
            delay: 1,
        }
    }
}
impl Retry {
    pub fn schedule(&mut self, now: Instant) {
        self.after = Some(now + Duration::from_secs(self.delay));
        self.delay = (self.delay * 2).min(15);
    }
    pub fn healthy(&mut self) {
        self.delay = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_can_stall_independently_of_video_and_silence_still_counts() {
        let start = Instant::now();
        let mut progress = Progress::new(start);
        assert!(progress.stalled(start, 1, 1, 1, 1, true).is_none());
        for second in 1..=20 {
            assert!(
                progress
                    .stalled(
                        start + Duration::from_secs(second),
                        second * 30,
                        second * 100,
                        second * 30,
                        second * 100,
                        true
                    )
                    .is_none()
            );
        }
        assert_eq!(
            progress.stalled(start + Duration::from_secs(26), 780, 2000, 780, 2600, true),
            Some("O áudio parou de receber dados")
        );
    }
    #[test]
    fn video_stall_is_detected_but_brief_ice_loss_and_audio_disabled_are_allowed() {
        let start = Instant::now();
        let mut progress = Progress::new(start);
        assert!(progress.stalled(start, 1, 0, 1, 0, true).is_none());
        assert!(
            progress
                .stalled(start + Duration::from_secs(2), 30, 0, 30, 0, false)
                .is_none()
        );
        assert!(
            progress
                .stalled(start + Duration::from_secs(4), 60, 0, 60, 0, true)
                .is_none()
        );
        assert_eq!(
            progress.stalled(start + Duration::from_secs(10), 60, 0, 90, 0, true),
            Some("O vídeo parou de receber quadros")
        );
    }
    #[test]
    fn static_screens_do_not_trigger_reconnections_and_initial_negotiation_has_time() {
        let start = Instant::now();
        let mut progress = Progress::new(start);
        assert!(progress.stalled(start, 0, 0, 0, 0, false).is_none());
        assert!(
            progress
                .stalled(start + Duration::from_secs(15), 0, 0, 0, 0, false)
                .is_none()
        );
        assert!(
            progress
                .stalled(start + Duration::from_secs(20), 1, 0, 1, 0, true)
                .is_none()
        );
        assert!(
            progress
                .stalled(start + Duration::from_secs(180), 1, 0, 1, 0, true)
                .is_none()
        );
    }
    #[test]
    fn an_audio_source_without_packets_during_silence_does_not_trigger_retries() {
        let start = Instant::now();
        let mut progress = Progress::new(start);
        assert!(progress.stalled(start, 1, 1, 1, 1, true).is_none());
        assert!(
            progress
                .stalled(start + Duration::from_secs(60), 1800, 1, 1800, 1, true)
                .is_none()
        );
    }
    #[test]
    fn retries_wait_back_off_and_reset_after_a_healthy_session() {
        let now = Instant::now();
        let mut retry = Retry::default();
        for delay in [1, 2, 4, 8, 15, 15] {
            retry.schedule(now);
            assert_eq!(retry.after, Some(now + Duration::from_secs(delay)));
        }
        retry.healthy();
        retry.schedule(now);
        assert_eq!(retry.after, Some(now + Duration::from_secs(1)));
    }
}
