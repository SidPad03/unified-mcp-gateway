//! The moments worth interrupting someone for.
//!
//! This decides *what* went wrong and *whether it is worth saying yet*. The app
//! decides how to say it — a macOS notification — and whether the person wants
//! to hear it at all. The rules live here, beside the state they watch, so the
//! grace period, the throttle and the question of what counts as a failure are
//! tested on the Linux CI runner like everything else in the core, rather than
//! only by someone unplugging their network.
//!
//! The emitter feeds [`AlertTracker::update`] on every tick: the snapshot when
//! it changed, the calls that arrived, and the time. Time is passed in rather
//! than read, which is what makes the timers testable, and it is a monotonic
//! clock that stops while the Mac sleeps — so closing the lid for the night is
//! not "disconnected for eight hours".

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::backends::BackendStatus;
use crate::logbuf::{CallStatus, ToolCall};
use crate::state::{ConnState, Snapshot};

/// How long the tunnel may be down before it is news.
///
/// A gateway redeploy, a Wi-Fi hop and a wake from sleep all drop the
/// connection for a few seconds and heal on their own. None of those should
/// reach the notification centre; a gateway that is still unreachable half a
/// minute later should.
pub const CONNECTION_GRACE: Duration = Duration::from_secs(30);

/// How soon a backend that keeps failing may be reported again.
///
/// The supervisor retries with a backoff that tops out at thirty seconds, so a
/// backend with a typo in its command fails twice a minute for as long as the
/// app runs. The first failure is news; the hundredth is not.
pub const BACKEND_REPEAT: Duration = Duration::from_secs(10 * 60);

/// How long failed tool calls are collected before they are reported together.
pub const CALLS_BATCH: Duration = Duration::from_secs(5);
/// The least time between two reports of failed tool calls.
pub const CALLS_WINDOW: Duration = Duration::from_secs(60);

/// Notifications keep their text in Notification Center; an error message is
/// not a log page.
const DETAIL_MAX_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Could not start: bad command, refused connection, failed handshake.
    BackendFailed,
    /// Was running, and the process exited on its own.
    BackendCrashed,
    /// Ready again after a failure that was reported.
    BackendRecovered,
    ConnectionLost,
    /// Connected again after a loss that was reported.
    ConnectionRestored,
    CallsFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alert {
    pub kind: AlertKind,
    /// The backend, or the tool of the latest failed call. Empty for the
    /// connection.
    pub subject: String,
    /// The error, redacted and shortened.
    pub detail: Option<String>,
    /// Restarts so far for a crashed backend; failed calls in the batch.
    pub count: u64,
}

struct BackendTrack {
    last: BackendStatus,
    /// A failure was reported and has not been followed by a recovery.
    reported: bool,
    /// When a failure was last reported. Kept through a recovery: the repeat
    /// interval is about how often this backend's failures are news, not about
    /// whether it came back in between.
    last_report: Option<Instant>,
}

#[derive(Default)]
struct ConnectionTrack {
    down_since: Option<Instant>,
    reported: bool,
    last_error: Option<String>,
    attempt: u32,
}

#[derive(Default)]
struct CallsTrack {
    pending: u64,
    first_pending: Option<Instant>,
    latest: Option<(String, Option<String>)>,
    last_report: Option<Instant>,
    /// Request ids already counted. A completion can be emitted more than once
    /// and must not be counted twice; bounded, oldest out first.
    counted: HashSet<String>,
    order: VecDeque<String>,
}

const COUNTED_MAX: usize = 512;

#[derive(Default)]
pub struct AlertTracker {
    backends: HashMap<String, BackendTrack>,
    connection: ConnectionTrack,
    calls: CallsTrack,
}

impl AlertTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything worth reporting as of `now`.
    ///
    /// Call it on every tick, with or without a new snapshot: the timers only
    /// fire when asked.
    pub fn update(
        &mut self,
        snapshot: Option<&Snapshot>,
        calls: &[ToolCall],
        now: Instant,
    ) -> Vec<Alert> {
        let mut alerts = Vec::new();
        if let Some(snapshot) = snapshot {
            self.observe_backends(snapshot, now, &mut alerts);
            self.observe_connection(snapshot, now, &mut alerts);
        }
        self.observe_calls(calls, now);
        self.fire_timers(now, &mut alerts);
        alerts
    }

    fn observe_backends(&mut self, snapshot: &Snapshot, now: Instant, alerts: &mut Vec<Alert>) {
        let present: HashSet<&str> = snapshot.backends.iter().map(|b| b.name.as_str()).collect();
        self.backends
            .retain(|name, _| present.contains(name.as_str()));

        for backend in &snapshot.backends {
            let track = self
                .backends
                .entry(backend.name.clone())
                .or_insert(BackendTrack {
                    last: BackendStatus::Starting,
                    reported: false,
                    last_report: None,
                });

            match backend.status {
                BackendStatus::Failed | BackendStatus::Crashed if track.last != backend.status => {
                    // One failure report per interval, whether or not it came
                    // back in between. A server that reaches Ready and crashes
                    // a few seconds later does so every thirty seconds, and
                    // clearing the throttle on each recovery made that two
                    // banners a cycle for as long as it went on.
                    let quiet = track
                        .last_report
                        .is_some_and(|at| now.duration_since(at) < BACKEND_REPEAT);
                    if !quiet {
                        alerts.push(Alert {
                            kind: if backend.status == BackendStatus::Crashed {
                                AlertKind::BackendCrashed
                            } else {
                                AlertKind::BackendFailed
                            },
                            subject: backend.name.clone(),
                            detail: backend.error.as_deref().map(shorten),
                            count: u64::from(backend.restarts),
                        });
                        track.reported = true;
                        track.last_report = Some(now);
                    }
                }
                // Only a reported failure has a recovery worth announcing; one
                // that stayed quiet under the throttle says nothing either way.
                BackendStatus::Ready if track.reported => {
                    alerts.push(Alert {
                        kind: AlertKind::BackendRecovered,
                        subject: backend.name.clone(),
                        detail: None,
                        count: 0,
                    });
                    track.reported = false;
                }
                // Turned off on purpose: whatever was wrong is no longer
                // something to announce the end of.
                BackendStatus::Disabled => {
                    track.reported = false;
                    track.last_report = None;
                }
                _ => {}
            }
            track.last = backend.status;
        }
    }

    fn observe_connection(&mut self, snapshot: &Snapshot, now: Instant, alerts: &mut Vec<Alert>) {
        let connection = &snapshot.connection;
        let track = &mut self.connection;
        match connection.state {
            // Signed out, or not set up yet. Nothing is supposed to be
            // connected, so nothing is lost.
            ConnState::Idle => *track = ConnectionTrack::default(),
            ConnState::Connected => {
                if track.reported {
                    alerts.push(Alert {
                        kind: AlertKind::ConnectionRestored,
                        subject: String::new(),
                        detail: None,
                        count: 0,
                    });
                }
                *track = ConnectionTrack::default();
            }
            ConnState::Connecting | ConnState::Reconnecting | ConnState::Error => {
                track.down_since.get_or_insert(now);
                if connection.last_error.is_some() {
                    track.last_error = connection.last_error.clone();
                }
                track.attempt = connection.attempt;
            }
        }
    }

    fn observe_calls(&mut self, calls: &[ToolCall], now: Instant) {
        let track = &mut self.calls;
        for call in calls {
            if call.status != CallStatus::Error || track.counted.contains(&call.request_id) {
                continue;
            }
            track.counted.insert(call.request_id.clone());
            track.order.push_back(call.request_id.clone());
            while track.order.len() > COUNTED_MAX {
                if let Some(old) = track.order.pop_front() {
                    track.counted.remove(&old);
                }
            }
            track.pending += 1;
            track.first_pending.get_or_insert(now);
            track.latest = Some((call.tool.clone(), call.error.as_deref().map(shorten)));
        }
    }

    fn fire_timers(&mut self, now: Instant, alerts: &mut Vec<Alert>) {
        let connection = &mut self.connection;
        if let Some(since) = connection.down_since {
            if !connection.reported && now.duration_since(since) >= CONNECTION_GRACE {
                alerts.push(Alert {
                    kind: AlertKind::ConnectionLost,
                    subject: String::new(),
                    detail: connection.last_error.as_deref().map(shorten),
                    count: u64::from(connection.attempt),
                });
                connection.reported = true;
            }
        }

        let calls = &mut self.calls;
        if let Some(first) = calls.first_pending {
            let batched = now.duration_since(first) >= CALLS_BATCH;
            let spaced = calls
                .last_report
                .is_none_or(|at| now.duration_since(at) >= CALLS_WINDOW);
            if batched && spaced {
                let (tool, error) = calls.latest.take().unwrap_or_default();
                alerts.push(Alert {
                    kind: AlertKind::CallsFailed,
                    subject: tool,
                    detail: error,
                    count: calls.pending,
                });
                calls.pending = 0;
                calls.first_pending = None;
                calls.last_report = Some(now);
            }
        }
    }
}

/// Redacted, on one line, and short enough for a notification.
fn shorten(text: &str) -> String {
    let redacted = crate::redact::redact(text);
    let line = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= DETAIL_MAX_CHARS {
        return line;
    }
    let mut cut: String = line.chars().take(DETAIL_MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::BackendView;
    use crate::config::{Config, ConfigView};
    use crate::state::{ConnectionStatus, Stats};

    fn backend(
        name: &str,
        status: BackendStatus,
        error: Option<&str>,
        restarts: u32,
    ) -> BackendView {
        BackendView {
            name: name.into(),
            transport: "stdio".into(),
            enabled: status != BackendStatus::Disabled,
            status,
            error: error.map(str::to_string),
            pid: None,
            started_at: None,
            uptime_secs: None,
            restarts,
            tool_count: 0,
            command: None,
            args: vec![],
            url: None,
            env: vec![],
            headers: vec![],
            tools: vec![],
        }
    }

    fn snapshot(state: ConnState, backends: Vec<BackendView>) -> Snapshot {
        Snapshot {
            connection: ConnectionStatus {
                state,
                last_error: (state == ConnState::Reconnecting)
                    .then(|| "IO error: Connection refused (os error 61)".to_string()),
                attempt: 3,
                ..Default::default()
            },
            backends,
            config: ConfigView::new(
                &Config::default(),
                std::path::Path::new("/tmp/c.toml"),
                true,
            ),
            stats: Stats {
                tools_registered: 0,
                backends_ready: 0,
                backends_total: 0,
                calls_total: 0,
                calls_errors: 0,
                log_lines_dropped: 0,
            },
            generation: 1,
            version: "test".into(),
            uptime_secs: 0,
        }
    }

    fn connected(backends: Vec<BackendView>) -> Snapshot {
        snapshot(ConnState::Connected, backends)
    }

    fn kinds(alerts: &[Alert]) -> Vec<AlertKind> {
        alerts.iter().map(|a| a.kind).collect()
    }

    fn call(id: &str, status: CallStatus, error: Option<&str>) -> ToolCall {
        ToolCall {
            seq: 1,
            request_id: id.into(),
            tool: "obsidian__get_note".into(),
            backend: Some("obsidian".into()),
            started_at: "2026-09-16T00:00:00Z".into(),
            duration_ms: Some(5),
            status,
            error: error.map(str::to_string),
        }
    }

    // ── Backends ────────────────────────────────────────────────────────

    #[test]
    fn a_backend_that_fails_to_start_is_reported_with_its_error() {
        let mut t = AlertTracker::new();
        let at = Instant::now();
        let starting = connected(vec![backend("blender", BackendStatus::Starting, None, 0)]);
        assert!(t.update(Some(&starting), &[], at).is_empty());

        let failed = connected(vec![backend(
            "blender",
            BackendStatus::Failed,
            Some("Failed to spawn 'uvx': No such file or directory"),
            0,
        )]);
        let alerts = t.update(Some(&failed), &[], at);
        assert_eq!(
            alerts,
            vec![Alert {
                kind: AlertKind::BackendFailed,
                subject: "blender".into(),
                detail: Some("Failed to spawn 'uvx': No such file or directory".into()),
                count: 0,
            }]
        );
    }

    #[test]
    fn a_crash_is_a_crash_and_carries_the_restart_count() {
        let mut t = AlertTracker::new();
        let at = Instant::now();
        t.update(
            Some(&connected(vec![backend(
                "fs",
                BackendStatus::Ready,
                None,
                0,
            )])),
            &[],
            at,
        );
        let alerts = t.update(
            Some(&connected(vec![backend(
                "fs",
                BackendStatus::Crashed,
                Some("Process exited (exit status: 1)"),
                2,
            )])),
            &[],
            at,
        );
        assert_eq!(kinds(&alerts), vec![AlertKind::BackendCrashed]);
        assert_eq!(alerts[0].count, 2);
    }

    /// The supervisor retries a broken backend with backoff, so it cycles
    /// starting → failed for as long as the app runs.
    #[test]
    fn a_backend_failing_in_a_loop_is_reported_once_per_interval() {
        let mut t = AlertTracker::new();
        let start = Instant::now();
        let failed = connected(vec![backend("x", BackendStatus::Failed, Some("boom"), 0)]);
        let starting = connected(vec![backend("x", BackendStatus::Starting, None, 0)]);

        let mut reported = 0;
        let mut at = start;
        // Twenty-one retries over ten minutes and a half.
        for _ in 0..21 {
            reported += t.update(Some(&starting), &[], at).len();
            reported += t.update(Some(&failed), &[], at).len();
            at += Duration::from_secs(30);
        }
        assert_eq!(
            reported, 2,
            "once at the start, once more after ten minutes"
        );
    }

    #[test]
    fn recovery_is_reported_only_after_a_reported_failure() {
        let mut t = AlertTracker::new();
        let at = Instant::now();
        // Starting → Ready with nothing wrong in between: silence.
        assert!(t
            .update(
                Some(&connected(vec![backend(
                    "a",
                    BackendStatus::Ready,
                    None,
                    0
                )])),
                &[],
                at
            )
            .is_empty());

        t.update(
            Some(&connected(vec![backend(
                "a",
                BackendStatus::Crashed,
                None,
                1,
            )])),
            &[],
            at,
        );
        t.update(
            Some(&connected(vec![backend(
                "a",
                BackendStatus::Starting,
                None,
                1,
            )])),
            &[],
            at,
        );
        let alerts = t.update(
            Some(&connected(vec![backend(
                "a",
                BackendStatus::Ready,
                None,
                1,
            )])),
            &[],
            at,
        );
        assert_eq!(kinds(&alerts), vec![AlertKind::BackendRecovered]);

        // A crash straight after is inside the repeat interval: quiet, and so
        // is the recovery that follows it.
        let crashed = connected(vec![backend("a", BackendStatus::Crashed, None, 2)]);
        let ready = connected(vec![backend("a", BackendStatus::Ready, None, 2)]);
        assert!(t.update(Some(&crashed), &[], at).is_empty());
        assert!(t.update(Some(&ready), &[], at).is_empty());

        // Past it, a crash is news again.
        let later = at + BACKEND_REPEAT;
        assert_eq!(
            kinds(&t.update(Some(&crashed), &[], later)),
            vec![AlertKind::BackendCrashed]
        );
    }

    /// Up for a few seconds, then down, every thirty seconds: the loop a
    /// server with a bad dependency gets into. Two notifications per interval
    /// — the crash and the recovery — not two per cycle.
    #[test]
    fn a_crash_and_recover_loop_is_throttled() {
        let mut t = AlertTracker::new();
        let start = Instant::now();
        let crashed = connected(vec![backend(
            "a",
            BackendStatus::Crashed,
            Some("exit 1"),
            0,
        )]);
        let starting = connected(vec![backend("a", BackendStatus::Starting, None, 0)]);
        let ready = connected(vec![backend("a", BackendStatus::Ready, None, 0)]);
        t.update(Some(&ready), &[], start);

        let mut seen = Vec::new();
        let mut at = start;
        // Twenty-five minutes of it.
        for _ in 0..50 {
            for snapshot in [&crashed, &starting, &ready] {
                seen.extend(kinds(&t.update(Some(snapshot), &[], at)));
            }
            at += Duration::from_secs(30);
        }
        assert_eq!(
            seen,
            [AlertKind::BackendCrashed, AlertKind::BackendRecovered].repeat(3),
            "at 0, 10 and 20 minutes"
        );
    }

    /// A restart the user asked for passes through `stopped`, and disabling a
    /// backend is a decision, not a failure. Neither says anything.
    #[test]
    fn deliberate_stops_are_not_failures() {
        let mut t = AlertTracker::new();
        let at = Instant::now();
        for status in [
            BackendStatus::Ready,
            BackendStatus::Stopped,
            BackendStatus::Starting,
            BackendStatus::Ready,
            BackendStatus::Disabled,
        ] {
            assert!(t
                .update(
                    Some(&connected(vec![backend("a", status, None, 0)])),
                    &[],
                    at
                )
                .is_empty());
        }
    }

    #[test]
    fn a_removed_backend_is_forgotten() {
        let mut t = AlertTracker::new();
        let at = Instant::now();
        t.update(
            Some(&connected(vec![backend(
                "a",
                BackendStatus::Failed,
                None,
                0,
            )])),
            &[],
            at,
        );
        t.update(Some(&connected(vec![])), &[], at);
        // Added back under the same name and failing: reported, not throttled
        // by the backend that used to have that name.
        let alerts = t.update(
            Some(&connected(vec![backend(
                "a",
                BackendStatus::Failed,
                None,
                0,
            )])),
            &[],
            at,
        );
        assert_eq!(kinds(&alerts), vec![AlertKind::BackendFailed]);
    }

    #[test]
    fn an_error_is_redacted_and_shortened() {
        let mut t = AlertTracker::new();
        let at = Instant::now();
        let long = format!("Authorization: Bearer abcdefghijklmnop {}", "x".repeat(400));
        let alerts = t.update(
            Some(&connected(vec![backend(
                "a",
                BackendStatus::Failed,
                Some(&long),
                0,
            )])),
            &[],
            at,
        );
        let detail = alerts[0].detail.as_deref().unwrap();
        assert!(!detail.contains("abcdefghijklmnop"), "{detail}");
        assert!(detail.chars().count() <= DETAIL_MAX_CHARS);
    }

    // ── Connection ──────────────────────────────────────────────────────

    #[test]
    fn a_short_drop_says_nothing() {
        let mut t = AlertTracker::new();
        let start = Instant::now();
        t.update(Some(&connected(vec![])), &[], start);
        t.update(Some(&snapshot(ConnState::Reconnecting, vec![])), &[], start);
        assert!(t
            .update(None, &[], start + Duration::from_secs(10))
            .is_empty());
        assert!(t
            .update(
                Some(&connected(vec![])),
                &[],
                start + Duration::from_secs(12)
            )
            .is_empty());
        // Well past the grace period now, and connected: still nothing.
        assert!(t
            .update(None, &[], start + Duration::from_secs(120))
            .is_empty());
    }

    #[test]
    fn a_long_drop_is_reported_once_then_its_end() {
        let mut t = AlertTracker::new();
        let start = Instant::now();
        t.update(Some(&connected(vec![])), &[], start);
        t.update(Some(&snapshot(ConnState::Reconnecting, vec![])), &[], start);

        // The timer fires on a tick with no new snapshot.
        let alerts = t.update(None, &[], start + CONNECTION_GRACE);
        assert_eq!(kinds(&alerts), vec![AlertKind::ConnectionLost]);
        assert_eq!(
            alerts[0].detail.as_deref(),
            Some("IO error: Connection refused (os error 61)")
        );
        assert_eq!(alerts[0].count, 3);

        // Retries keep coming; none of them is news.
        for s in 31..300 {
            assert!(t
                .update(
                    Some(&snapshot(ConnState::Reconnecting, vec![])),
                    &[],
                    start + Duration::from_secs(s)
                )
                .is_empty());
        }

        let alerts = t.update(
            Some(&connected(vec![])),
            &[],
            start + Duration::from_secs(301),
        );
        assert_eq!(kinds(&alerts), vec![AlertKind::ConnectionRestored]);
    }

    /// A Mac that boots with the gateway unreachable never connects at all,
    /// and that is worth hearing about too.
    #[test]
    fn never_connecting_is_reported_after_the_grace_period() {
        let mut t = AlertTracker::new();
        let start = Instant::now();
        t.update(Some(&snapshot(ConnState::Connecting, vec![])), &[], start);
        assert!(t
            .update(None, &[], start + Duration::from_secs(29))
            .is_empty());
        assert_eq!(
            kinds(&t.update(None, &[], start + Duration::from_secs(30))),
            vec![AlertKind::ConnectionLost]
        );
    }

    #[test]
    fn signing_out_cancels_a_pending_report() {
        let mut t = AlertTracker::new();
        let start = Instant::now();
        t.update(Some(&snapshot(ConnState::Reconnecting, vec![])), &[], start);
        t.update(
            Some(&snapshot(ConnState::Idle, vec![])),
            &[],
            start + Duration::from_secs(5),
        );
        assert!(t
            .update(None, &[], start + Duration::from_secs(60))
            .is_empty());
    }

    // ── Tool calls ──────────────────────────────────────────────────────

    #[test]
    fn failed_calls_are_batched_and_spaced() {
        let mut t = AlertTracker::new();
        let start = Instant::now();

        assert!(t
            .update(
                None,
                &[call("1", CallStatus::Error, Some("timed out"))],
                start
            )
            .is_empty());
        // Completions and successes do not add to it; a repeat of the same
        // request does not count twice.
        t.update(
            None,
            &[
                call("1", CallStatus::Error, Some("timed out")),
                call("2", CallStatus::Ok, None),
                call("3", CallStatus::Running, None),
                call(
                    "4",
                    CallStatus::Error,
                    Some("Backend 'obsidian' is not running"),
                ),
            ],
            start + Duration::from_secs(1),
        );

        let alerts = t.update(None, &[], start + CALLS_BATCH);
        assert_eq!(kinds(&alerts), vec![AlertKind::CallsFailed]);
        assert_eq!(alerts[0].count, 2);
        assert_eq!(alerts[0].subject, "obsidian__get_note");
        assert_eq!(
            alerts[0].detail.as_deref(),
            Some("Backend 'obsidian' is not running")
        );

        // More failures straight after wait out the window, then arrive as one.
        let later = start + CALLS_BATCH + Duration::from_secs(1);
        t.update(None, &[call("5", CallStatus::Error, Some("x"))], later);
        t.update(None, &[call("6", CallStatus::Error, Some("y"))], later);
        assert!(t
            .update(None, &[], later + Duration::from_secs(30))
            .is_empty());
        let alerts = t.update(None, &[], start + CALLS_BATCH + CALLS_WINDOW);
        assert_eq!(alerts[0].count, 2);
    }

    #[test]
    fn alerts_serialize_the_way_the_app_reads_them() {
        let alert = Alert {
            kind: AlertKind::ConnectionLost,
            subject: String::new(),
            detail: Some("refused".into()),
            count: 4,
        };
        assert_eq!(
            serde_json::to_value(&alert).unwrap(),
            serde_json::json!({"kind": "connection_lost", "subject": "", "detail": "refused", "count": 4})
        );
    }
}
