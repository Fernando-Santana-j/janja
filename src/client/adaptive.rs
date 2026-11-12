use super::Settings;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub(super) const MIN_JITTER_MS: u32 = 40;

pub(super) struct Jitter {
    pub ms: u32,
    late: u64,
    quiet: Duration,
}
impl Default for Jitter {
    fn default() -> Self {
        Self {
            ms: MIN_JITTER_MS,
            late: 0,
            quiet: Duration::ZERO,
        }
    }
}
impl Jitter {
    pub fn next(&mut self, elapsed: Duration, late: u64, jitter_ns: u64) -> Option<u32> {
        let floor =
            (jitter_ns.saturating_mul(4) / 1_000_000 + 20).clamp(MIN_JITTER_MS as u64, 150) as u32;
        let next = if late > self.late {
            self.quiet = Duration::ZERO;
            self.ms.saturating_add(20).max(floor).min(150)
        } else if floor > self.ms {
            self.quiet = Duration::ZERO;
            floor
        } else {
            self.quiet += elapsed.min(Duration::from_secs(5));
            if self.quiet >= Duration::from_secs(10) {
                self.quiet = Duration::ZERO;
                self.ms.saturating_sub(10).max(floor)
            } else {
                self.ms
            }
        };
        self.late = late;
        if next == self.ms {
            return None;
        }
        self.ms = next;
        Some(next)
    }
}

struct LossReport {
    sequence: u32,
    bad: u8,
    updated: Instant,
}
#[derive(Default)]
pub(super) struct LossReports(HashMap<String, LossReport>);
impl LossReports {
    pub fn observe(&mut self, stats: &gst::StructureRef, now: Instant) {
        for (id, value) in stats.iter() {
            let Ok(Some(report)) = value.get::<gst::Structure>() else {
                continue;
            };
            let Some(fraction) = report
                .get::<f64>("fraction-lost")
                .ok()
                .flatten()
                .filter(|f| f.is_finite() && (0.0..=1.0).contains(f))
            else {
                continue;
            };
            let Some(sequence) = report
                .get::<gst::Structure>("gst-rtpsource-stats")
                .ok()
                .flatten()
                .and_then(|r| r.get::<u32>("rb-exthighestseq").ok().flatten())
            else {
                continue;
            };
            if self.0.get(id).is_some_and(|r| r.sequence == sequence) {
                continue;
            }
            // A peer carries at most audio and video; bound SSRC history after restarts.
            if self.0.len() >= 16 {
                self.0.retain(|_, r| {
                    now.saturating_duration_since(r.updated) <= Duration::from_secs(10)
                });
            }
            if self.0.len() >= 16 && !self.0.contains_key(id) {
                continue;
            }
            let previous = self
                .0
                .get(id)
                .filter(|r| now.saturating_duration_since(r.updated) <= Duration::from_secs(10));
            let bad = if fraction >= 0.10 {
                previous.map_or(1, |r| r.bad.saturating_add(1))
            } else {
                0
            };
            self.0.insert(
                id.to_owned(),
                LossReport {
                    sequence,
                    bad,
                    updated: now,
                },
            );
        }
    }
    pub fn congested(&self, now: Instant) -> bool {
        self.0.values().any(|r| {
            r.bad >= 2 && now.saturating_duration_since(r.updated) <= Duration::from_secs(10)
        })
    }
}

pub(super) struct Support {
    pub bitrate: bool,
    pub resize: bool,
}

pub(super) struct Controller {
    local: Duration,
    network: Duration,
    cooldown: Duration,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            local: Duration::ZERO,
            network: Duration::ZERO,
            cooldown: Duration::from_secs(6),
        }
    }
}
impl Controller {
    pub fn next(
        &mut self,
        current: &Settings,
        elapsed: Duration,
        local: bool,
        network: bool,
        support: Support,
    ) -> Option<(Settings, &'static str)> {
        // A delayed engine poll is one observation, not proof of sustained pressure.
        if !(Duration::from_secs(1)..=Duration::from_secs(5)).contains(&elapsed) {
            self.local = Duration::ZERO;
            self.network = Duration::ZERO;
            return None;
        }
        self.cooldown = self.cooldown.saturating_sub(elapsed);
        self.local = if local {
            self.local + elapsed
        } else {
            Duration::ZERO
        };
        self.network = if network {
            self.network + elapsed
        } else {
            Duration::ZERO
        };
        if !self.cooldown.is_zero() {
            return None;
        }
        let local = self.local >= Duration::from_secs(6);
        let network = self.network >= Duration::from_secs(6);
        if !local && !network {
            return None;
        }
        let mut next = current.clone();
        if network && !local && support.bitrate && current.bitrate > 1000 {
            next.bitrate = (current.bitrate * 3 / 4).max(1000);
        } else if current.fps > 30 {
            next.fps = 30;
        } else if support.resize && current.height > 720 {
            next.height = if current.height > 1080 { 1080 } else { 720 };
        } else if current.fps > 15 {
            next.fps = 15;
        } else {
            return None;
        }
        self.local = Duration::ZERO;
        self.network = Duration::ZERO;
        self.cooldown = Duration::from_secs(10);
        Some((
            next,
            if local {
                "sobrecarga local"
            } else {
                "perdas na rede"
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn jitter_starts_low_recovers_delay_and_slowly_returns_to_low_latency() {
        let mut jitter = super::Jitter::default();
        let tick = std::time::Duration::from_secs(2);
        assert_eq!(jitter.ms, 40);
        assert_eq!(jitter.next(tick, 0, 1_000_000), None);
        assert_eq!(jitter.next(tick, 1, 1_000_000), Some(60));
        assert_eq!(jitter.next(tick, 1, 40_000_000), Some(150));
        for _ in 0..4 {
            assert_eq!(jitter.next(tick, 1, 1_000_000), None);
        }
        assert_eq!(jitter.next(tick, 1, 1_000_000), Some(140));
        for _ in 0..100 {
            jitter.next(tick, 1, 1_000_000);
        }
        assert_eq!(jitter.ms, 40);
        assert_eq!(jitter.next(tick, 0, u64::MAX), Some(150));
        assert_eq!(jitter.next(tick, u64::MAX, 0), None);
    }

    use super::*;
    fn loss_report(sequence: u32, loss: f64) -> gst::Structure {
        let source = gst::Structure::builder("source")
            .field("rb-exthighestseq", &sequence)
            .build();
        let report = gst::Structure::builder("remote-inbound-rtp")
            .field("fraction-lost", &loss)
            .field("gst-rtpsource-stats", &source)
            .build();
        gst::Structure::builder("application/x-webrtc-stats")
            .field("video", &report)
            .build()
    }
    #[test]
    fn loss_requires_fresh_reports_recovers_and_expires() {
        gst::init().unwrap();
        let now = Instant::now();
        let mut reports = LossReports::default();
        reports.observe(&loss_report(100, 0.2), now);
        for _ in 0..10 {
            reports.observe(&loss_report(100, 0.2), now);
        }
        assert!(
            !reports.congested(now),
            "re-reading the same RTCP report is not sustained loss"
        );
        reports.observe(&loss_report(200, 0.2), now + Duration::from_secs(2));
        assert!(reports.congested(now + Duration::from_secs(3)));
        assert!(!reports.congested(now + Duration::from_secs(13)));
        reports.observe(&loss_report(300, 0.0), now + Duration::from_secs(4));
        assert!(!reports.congested(now + Duration::from_secs(4)));
        for loss in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            reports.observe(&loss_report(400, loss), now + Duration::from_secs(5));
        }
        assert!(!reports.congested(now + Duration::from_secs(5)));
        reports.observe(&loss_report(500, 0.2), now + Duration::from_secs(20));
        assert!(!reports.congested(now + Duration::from_secs(20)));
    }
    #[test]
    fn healthy_audio_reports_do_not_erase_video_loss() {
        gst::init().unwrap();
        let now = Instant::now();
        let mut reports = LossReports::default();
        reports.observe(&loss_report(100, 0.2), now);
        let mut audio = loss_report(200, 0.0);
        let report = audio.get_value("video").unwrap().clone();
        audio.remove_field("video");
        audio.set_value("audio", report);
        reports.observe(&audio, now + Duration::from_secs(1));
        reports.observe(&loss_report(200, 0.2), now + Duration::from_secs(2));
        reports.observe(&audio, now + Duration::from_secs(3));
        assert!(reports.congested(now + Duration::from_secs(3)));
    }
    fn observe(c: &mut Controller, s: &Settings, local: bool, network: bool) -> Option<Settings> {
        c.next(
            s,
            Duration::from_secs(2),
            local,
            network,
            Support {
                bitrate: true,
                resize: true,
            },
        )
        .map(|(s, _)| s)
    }
    #[test]
    fn sustained_overload_reduces_work_without_increasing_any_setting() {
        let mut c = Controller::default();
        let mut s = Settings {
            fps: 60,
            height: 1440,
            ..Settings::default()
        };
        let mut changes = Vec::new();
        for _ in 0..40 {
            if let Some(next) = observe(&mut c, &s, true, false) {
                assert!(next.fps <= s.fps && next.height <= s.height && next.bitrate <= s.bitrate);
                assert_eq!((next.audio, next.source), (s.audio, s.source));
                changes.push((next.height, next.fps));
                s = next;
            }
        }
        assert_eq!(changes, [(1440, 30), (1080, 30), (720, 30), (720, 15)]);
    }
    #[test]
    fn isolated_spike_idle_and_cooldown_do_not_oscillate() {
        let mut c = Controller::default();
        let s = Settings::default();
        for _ in 0..20 {
            assert!(observe(&mut c, &s, false, false).is_none());
        }
        assert!(observe(&mut c, &s, true, false).is_none());
        assert!(observe(&mut c, &s, false, false).is_none());
        assert!(observe(&mut c, &s, true, false).is_none());
        assert!(observe(&mut c, &s, true, false).is_none());
        let lower = observe(&mut c, &s, true, false).unwrap();
        for _ in 0..4 {
            assert!(observe(&mut c, &lower, true, false).is_none());
        }
    }
    #[test]
    fn network_reduces_bitrate_to_floor_before_video_and_respects_codec_support() {
        let mut c = Controller::default();
        let mut s = Settings {
            bitrate: 2000,
            ..Settings::default()
        };
        for _ in 0..100 {
            if let Some(next) = observe(&mut c, &s, false, true) {
                assert!(next.bitrate >= 1000);
                if s.bitrate > 1000 {
                    assert!(next.bitrate < s.bitrate);
                    assert_eq!((next.height, next.fps), (s.height, s.fps));
                }
                s = next;
            }
        }
        assert_eq!((s.bitrate, s.height, s.fps), (1000, 720, 15));
        let mut c = Controller::default();
        for _ in 0..10 {
            if let Some((next, _)) = c.next(
                &Settings::default(),
                Duration::from_secs(2),
                true,
                true,
                Support {
                    bitrate: false,
                    resize: false,
                },
            ) {
                assert_eq!(next.height, 1080);
                assert_eq!(next.bitrate, 8000);
                assert_eq!(next.fps, 15);
            }
        }
    }
    #[test]
    fn suspended_engine_does_not_count_one_sample_as_sustained_pressure() {
        let mut c = Controller::default();
        assert!(
            c.next(
                &Settings::default(),
                Duration::from_secs(90),
                true,
                true,
                Support {
                    bitrate: true,
                    resize: true
                }
            )
            .is_none()
        );
    }
}
