//! Bounded ring buffers for log lines and tool calls, plus the tracing layer
//! that feeds the first one.
//!
//! These are the source of truth. The webview holds no unbounded history: it
//! asks for a snapshot when a page mounts, then receives deltas on a ~100 ms
//! tick. That is why every record carries a `seq` — the shell keeps a cursor and
//! calls [`LogBuffer::since`] / [`CallBuffer::since`], so a backend that writes
//! ten thousand stderr lines in a second costs one IPC message, not ten
//! thousand.
//!
//! Mutation and the cursor interact in one place worth understanding:
//! completing a tool call **re-stamps** its `seq`, so an already-delivered
//! record shows up again in the next delta with its duration filled in. The
//! webview keys tool calls by `request_id` and merges, so a re-delivery updates
//! the row rather than duplicating it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

/// Roughly a screen-hour of a chatty backend.
pub const MAX_LOG_LINES: usize = 5_000;
/// Tool calls kept for the Activity page.
pub const MAX_CALLS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn from_tracing(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::TRACE => Self::Trace,
            tracing::Level::DEBUG => Self::Debug,
            tracing::Level::INFO => Self::Info,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::ERROR => Self::Error,
        }
    }

    /// Classify a line of a child process's stderr.
    ///
    /// MCP servers write anything from a banner to a stack trace here, with no
    /// agreed format. Guessing from keywords is imperfect but it is what makes
    /// the Logs page's level filter useful at all; the raw text is always shown
    /// unchanged.
    pub fn guess_from_stderr(line: &str) -> Self {
        let lower = line.to_ascii_lowercase();
        if lower.contains("error")
            || lower.contains("fatal")
            || lower.contains("panic")
            || lower.contains("traceback")
        {
            Self::Error
        } else if lower.contains("warn") {
            Self::Warn
        } else if lower.contains("debug") {
            Self::Debug
        } else {
            Self::Info
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub seq: u64,
    /// RFC 3339, UTC. The webview renders it in the local timezone.
    pub ts: String,
    pub level: LogLevel,
    /// `agent` for the app's own logs, otherwise the backend name.
    pub source: String,
    pub message: String,
}

pub struct LogBuffer {
    inner: Mutex<VecDeque<LogLine>>,
    next_seq: AtomicU64,
    /// Lines evicted by the ring. Surfaced so the Logs page can say so instead
    /// of silently showing a truncated history.
    dropped: AtomicU64,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(256)),
            next_seq: AtomicU64::new(1),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn push(&self, level: LogLevel, source: impl Into<String>, message: impl Into<String>) {
        let line = LogLine {
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            ts: crate::now_rfc3339(),
            level,
            source: source.into(),
            // Redact on the way in. A log line is written once and read many
            // times — including by "Export" and "Copy", which is exactly where
            // a leaked credential would escape the machine.
            message: crate::redact::redact(&message.into()),
        };
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.push_back(line);
        while guard.len() > MAX_LOG_LINES {
            guard.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Everything newer than `cursor`, and the cursor to pass next time.
    pub fn since(&self, cursor: u64) -> (Vec<LogLine>, u64) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let lines: Vec<LogLine> = guard.iter().filter(|l| l.seq > cursor).cloned().collect();
        let next = lines
            .last()
            .map(|l| l.seq)
            .unwrap_or_else(|| cursor.max(guard.back().map(|l| l.seq).unwrap_or(cursor)));
        (lines, next)
    }

    pub fn snapshot(&self) -> Vec<LogLine> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().cloned().collect()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.dropped.store(0, Ordering::Relaxed);
    }
}

// ── Tool calls ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CallStatus {
    Running,
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCall {
    pub seq: u64,
    pub request_id: String,
    /// Namespaced name as the gateway sent it, e.g. `blender__get_scene_info`.
    pub tool: String,
    /// The local backend it routed to, once known.
    pub backend: Option<String>,
    pub started_at: String,
    pub duration_ms: Option<u64>,
    pub status: CallStatus,
    pub error: Option<String>,
}

pub struct CallBuffer {
    inner: Mutex<VecDeque<ToolCall>>,
    next_seq: AtomicU64,
    total: AtomicU64,
    errors: AtomicU64,
}

impl Default for CallBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl CallBuffer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(128)),
            next_seq: AtomicU64::new(1),
            total: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    pub fn start(&self, request_id: &str, tool: &str, backend: Option<String>) {
        let call = ToolCall {
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            request_id: request_id.to_string(),
            tool: tool.to_string(),
            backend,
            started_at: crate::now_rfc3339(),
            duration_ms: None,
            status: CallStatus::Running,
            error: None,
        };
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.push_back(call);
        while guard.len() > MAX_CALLS {
            guard.pop_front();
        }
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    /// Close out a call **by request id**.
    ///
    /// The old TUI matched a completion to a call by tool name and took the
    /// first record with no duration yet. Two concurrent calls to the same tool
    /// therefore updated each other's row — defect #6. The gateway gives us a
    /// request id precisely so we do not have to guess.
    pub fn complete(&self, request_id: &str, duration_ms: u64, error: Option<String>) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Newest first: a call in flight is almost always near the back.
        if let Some(call) = guard.iter_mut().rev().find(|c| c.request_id == request_id) {
            call.seq = seq;
            call.duration_ms = Some(duration_ms);
            call.status = if error.is_some() {
                CallStatus::Error
            } else {
                CallStatus::Ok
            };
            call.error = error.as_deref().map(crate::redact::redact);
        }
        if error.is_some() {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Fill in the backend once routing has resolved it.
    pub fn set_backend(&self, request_id: &str, backend: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(call) = guard.iter_mut().rev().find(|c| c.request_id == request_id) {
            call.backend = Some(backend.to_string());
        }
    }

    pub fn since(&self, cursor: u64) -> (Vec<ToolCall>, u64) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut calls: Vec<ToolCall> = guard.iter().filter(|c| c.seq > cursor).cloned().collect();
        calls.sort_by_key(|c| c.seq);
        let next = calls.last().map(|c| c.seq).unwrap_or(cursor).max(cursor);
        (calls, next)
    }

    pub fn snapshot(&self) -> Vec<ToolCall> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().cloned().collect()
    }

    pub fn totals(&self) -> (u64, u64) {
        (
            self.total.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
        )
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

// ── tracing → LogBuffer ─────────────────────────────────────────────────

/// A `tracing` layer that mirrors every event into a [`LogBuffer`].
///
/// The app has no terminal, so `tracing`'s default writer goes nowhere. Wiring
/// the buffer in as a layer means an ordinary `tracing::warn!` anywhere in the
/// agent shows up on the Logs page, and nobody has to remember to log twice.
pub struct LogLayer {
    buffer: std::sync::Arc<LogBuffer>,
}

impl LogLayer {
    pub fn new(buffer: std::sync::Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S> tracing_subscriber::Layer<S> for LogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        if visitor.message.is_empty() {
            return;
        }
        let mut message = visitor.message;
        if !visitor.fields.is_empty() {
            message.push_str(" (");
            message.push_str(&visitor.fields.join(", "));
            message.push(')');
        }
        self.buffer.push(
            LogLevel::from_tracing(event.metadata().level()),
            "agent",
            message,
        );
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_buffer_is_bounded() {
        let buf = LogBuffer::new();
        for i in 0..MAX_LOG_LINES + 500 {
            buf.push(LogLevel::Info, "agent", format!("line {i}"));
        }
        assert_eq!(buf.snapshot().len(), MAX_LOG_LINES);
        assert_eq!(buf.dropped(), 500);
        // The oldest lines are the ones gone.
        assert_eq!(buf.snapshot()[0].message, "line 500");
    }

    #[test]
    fn deltas_do_not_repeat_lines() {
        let buf = LogBuffer::new();
        buf.push(LogLevel::Info, "agent", "one");
        buf.push(LogLevel::Info, "agent", "two");

        let (first, cursor) = buf.since(0);
        assert_eq!(first.len(), 2);

        let (empty, cursor2) = buf.since(cursor);
        assert!(empty.is_empty());
        assert_eq!(cursor2, cursor);

        buf.push(LogLevel::Warn, "blender", "three");
        let (next, _) = buf.since(cursor2);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].message, "three");
        assert_eq!(next[0].source, "blender");
    }

    #[test]
    fn log_lines_are_redacted_on_the_way_in() {
        let buf = LogBuffer::new();
        buf.push(
            LogLevel::Info,
            "agent",
            "connecting with token mcpgw_abcdef0123456789",
        );
        let line = &buf.snapshot()[0];
        assert!(!line.message.contains("mcpgw_abcdef"), "{}", line.message);
    }

    #[test]
    fn call_buffer_is_bounded() {
        let buf = CallBuffer::new();
        for i in 0..MAX_CALLS + 10 {
            buf.start(&format!("r{i}"), "b__t", Some("b".into()));
        }
        assert_eq!(buf.snapshot().len(), MAX_CALLS);
    }

    #[test]
    fn concurrent_calls_to_one_tool_do_not_overwrite_each_other() {
        // Defect #6: matching a completion by tool name updated the wrong row.
        let buf = CallBuffer::new();
        buf.start("req-a", "blender__render", Some("blender".into()));
        buf.start("req-b", "blender__render", Some("blender".into()));

        // The *second* call finishes first — the interleaving the old code got
        // wrong.
        buf.complete("req-b", 120, None);

        let calls = buf.snapshot();
        let a = calls.iter().find(|c| c.request_id == "req-a").unwrap();
        let b = calls.iter().find(|c| c.request_id == "req-b").unwrap();
        assert_eq!(a.status, CallStatus::Running, "req-a must still be running");
        assert_eq!(a.duration_ms, None);
        assert_eq!(b.status, CallStatus::Ok);
        assert_eq!(b.duration_ms, Some(120));

        buf.complete("req-a", 4000, Some("timed out".into()));
        let calls = buf.snapshot();
        let a = calls.iter().find(|c| c.request_id == "req-a").unwrap();
        assert_eq!(a.status, CallStatus::Error);
        assert_eq!(a.duration_ms, Some(4000));
        assert_eq!(a.error.as_deref(), Some("timed out"));
    }

    #[test]
    fn completing_a_call_re_delivers_it_in_the_next_delta() {
        let buf = CallBuffer::new();
        buf.start("r1", "b__t", None);
        let (first, cursor) = buf.since(0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].status, CallStatus::Running);

        buf.complete("r1", 42, None);
        let (second, _) = buf.since(cursor);
        assert_eq!(second.len(), 1, "the completion must reach the webview");
        assert_eq!(second[0].request_id, "r1");
        assert_eq!(second[0].duration_ms, Some(42));
    }

    #[test]
    fn completing_an_unknown_request_is_a_no_op() {
        let buf = CallBuffer::new();
        buf.complete("never-started", 1, None);
        assert!(buf.snapshot().is_empty());
    }

    #[test]
    fn stderr_levels_are_guessed_from_the_text() {
        assert_eq!(
            LogLevel::guess_from_stderr("Traceback (most recent call last):"),
            LogLevel::Error
        );
        assert_eq!(
            LogLevel::guess_from_stderr("WARNING: deprecated flag"),
            LogLevel::Warn
        );
        assert_eq!(
            LogLevel::guess_from_stderr("Listening on :3010"),
            LogLevel::Info
        );
    }
}
