pub mod recorder;
pub mod redactor;

pub use recorder::AuditRecorder;

/// One live-feed frame: the already-serialised JSON, plus the user it belongs
/// to so a subscriber can be filtered without re-parsing it.
///
/// The pair travels together because the frame is broadcast to every connected
/// dashboard and the socket has to decide, per subscriber, whether that
/// subscriber is allowed to see it. Sending only the string made that decision
/// impossible, so the feed forwarded every user's calls to every viewer.
#[derive(Clone, Debug)]
pub struct LiveEvent {
    pub user_id: Option<uuid::Uuid>,
    pub json: String,
}
