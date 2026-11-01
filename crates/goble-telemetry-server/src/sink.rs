//! Where a stored item goes besides the local store.
//!
//! Local disk is the source of truth: an item is written there first, and the
//! sink is a copy of it. That ordering is what lets a failed sink write be a
//! retry from the client's point of view instead of a lost event: nothing is
//! acknowledged until both writes are done, and a retry with the same
//! `event_id` overwrites the same object.
//!
//! The S3 layout is the one the Athena tables read:
//!
//! ```text
//! <prefix>/events/dt=YYYY-MM-DD/<event_id>.json
//! <prefix>/crashes/<fingerprint>/<report-id>.json
//! ```
//!
//! The day is the event's `occurred_at`, never the collector's clock: an event
//! queued offline for two days lands in the day it happened.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use goble_telemetry::CrashReport;

use crate::{sanitise, StoredEvent};

/// The bucket to copy into. Unset means the local store is the only sink.
pub const ENV_BUCKET: &str = "GOBLE_TELEMETRY_S3_BUCKET";
/// Where under the bucket the layout starts.
pub const ENV_PREFIX: &str = "GOBLE_TELEMETRY_S3_PREFIX";
/// Used when the bucket is set and the prefix is not.
pub const DEFAULT_PREFIX: &str = "raw";

/// A write or probe that did not happen.
///
/// Every failure is reduced to one of these labels before it reaches a log line
/// or an HTTP response, so a URL, a request id, or a credential can never leak
/// through an error string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkFailure {
    /// The credentials work but are not allowed to do this.
    AccessDenied,
    /// The bucket does not exist, which is a configuration mistake.
    NotFound,
    /// The endpoint could not be reached, or timed out.
    Unavailable,
    /// Anything else, including a body that would not serialise.
    Other,
}

impl SinkFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccessDenied => "access_denied",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::Other => "other",
        }
    }
}

/// The future a sink returns. Boxed because a sink is called from one generic
/// handler and implemented behind a trait object.
pub type SinkFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SinkFailure>> + Send + 'a>>;

/// A second place a stored item is copied to.
///
/// A sink is asked again on a retry, so each method must be safe to call with
/// the same id twice: the S3 keys are derived from the item's own ids.
pub trait Sink: std::fmt::Debug + Send + Sync {
    fn put_event<'a>(&'a self, event: &'a StoredEvent) -> SinkFuture<'a, ()>;
    fn put_crash<'a>(&'a self, report: &'a CrashReport) -> SinkFuture<'a, ()>;
    /// Report whether the sink can be used, without writing anything.
    fn probe<'a>(&'a self) -> SinkFuture<'a, ()>;
}

/// The object-storage sink.
#[derive(Debug, Clone)]
pub struct S3Sink {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
}

impl S3Sink {
    pub fn new(client: aws_sdk_s3::Client, bucket: impl Into<String>, prefix: &str) -> Self {
        Self {
            client,
            bucket: bucket.into(),
            prefix: normalise_prefix(prefix),
        }
    }

    /// The sink the environment describes, or `None` when no bucket is named.
    ///
    /// Credentials come from the standard AWS chain; this collector never
    /// reads a key or a secret itself.
    pub async fn from_env() -> Option<Self> {
        let bucket = std::env::var(ENV_BUCKET).ok()?;
        let bucket = bucket.trim();
        if bucket.is_empty() {
            return None;
        }

        let prefix = std::env::var(ENV_PREFIX).unwrap_or_else(|_| DEFAULT_PREFIX.to_owned());
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        Some(Self::new(aws_sdk_s3::Client::new(&config), bucket, &prefix))
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    async fn put_object(&self, key: String, body: Vec<u8>) -> Result<(), SinkFailure> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .content_type("application/json")
            .body(aws_sdk_s3::primitives::ByteStream::from(body))
            .send()
            .await
            .map(|_| ())
            .map_err(|error| {
                let failure = classify(&error);
                tracing::warn!(key = %key, reason = failure.as_str(), "the object store write failed");
                failure
            })
    }
}

impl Sink for S3Sink {
    fn put_event<'a>(&'a self, event: &'a StoredEvent) -> SinkFuture<'a, ()> {
        Box::pin(async move {
            let key = event_key(&self.prefix, event.occurred_at(), &event.envelope.event_id);
            let body = serde_json::to_vec(event).map_err(|error| {
                tracing::error!("could not encode an event for the object store: {error}");
                SinkFailure::Other
            })?;
            self.put_object(key, body).await
        })
    }

    fn put_crash<'a>(&'a self, report: &'a CrashReport) -> SinkFuture<'a, ()> {
        Box::pin(async move {
            let key = crash_key(&self.prefix, &report.fingerprint, &report.id);
            let body = serde_json::to_vec(report).map_err(|error| {
                tracing::error!("could not encode a crash report for the object store: {error}");
                SinkFailure::Other
            })?;
            self.put_object(key, body).await
        })
    }

    fn probe<'a>(&'a self) -> SinkFuture<'a, ()> {
        Box::pin(async move {
            // A `HEAD` on the bucket is the cheapest reachability check the
            // service offers; a policy that only allows writes answers
            // `access_denied`, which still proves the endpoint is reachable.
            self.client
                .head_bucket()
                .bucket(&self.bucket)
                .send()
                .await
                .map(|_| ())
                .map_err(|error| classify(&error))
        })
    }
}

/// The key one event is written under.
///
/// Public so the layout has one definition: the Athena tables in
/// `infra/telemetry/athena/` read exactly these paths.
pub fn event_key(prefix: &str, occurred_at: DateTime<Utc>, event_id: &str) -> String {
    let day = occurred_at.date_naive();
    join(prefix, &format!("events/dt={day}/{}.json", sanitise(event_id)))
}

/// The key one crash report is written under.
pub fn crash_key(prefix: &str, fingerprint: &str, report_id: &str) -> String {
    join(
        prefix,
        &format!("crashes/{}/{}.json", sanitise(fingerprint), sanitise(report_id)),
    )
}

fn join(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

fn normalise_prefix(prefix: &str) -> String {
    prefix.trim().trim_matches('/').to_owned()
}

/// Reduce an SDK error to a label. The error text is never returned to a
/// client, and the request id is never formatted into a response.
fn classify<E, R>(error: &aws_sdk_s3::error::SdkError<E, R>) -> SinkFailure
where
    E: std::error::Error + aws_sdk_s3::error::ProvideErrorMetadata + Send + Sync + 'static,
    R: std::fmt::Debug + Send + Sync + 'static,
{
    use aws_sdk_s3::error::SdkError;

    match error {
        SdkError::ServiceError(service) => match service.err().code() {
            Some("NoSuchBucket") => SinkFailure::NotFound,
            Some("AccessDenied" | "InvalidAccessKeyId" | "SignatureDoesNotMatch" | "ExpiredToken") => {
                SinkFailure::AccessDenied
            }
            Some("SlowDown" | "RequestTimeout" | "ServiceUnavailable") => SinkFailure::Unavailable,
            _ => SinkFailure::Other,
        },
        SdkError::TimeoutError(_) | SdkError::DispatchFailure(_) | SdkError::ResponseError(_) => {
            SinkFailure::Unavailable
        }
        _ => SinkFailure::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_telemetry::{Channel, ClientContext, Role, TelemetryEvent};
    use goble_telemetry::EventEnvelope;

    fn event_at(occurred_at: DateTime<Utc>, event_id: &str) -> StoredEvent {
        let context = ClientContext {
            app_version: "0.1.0".into(),
            channel: Channel::Stable,
            os: "macos".into(),
            os_version: None,
            arch: "aarch64".into(),
            install_id: "install".into(),
            link_id: None,
            role: Role::Desktop,
            session_id: "session".into(),
        };
        let mut envelope =
            EventEnvelope::new(context, TelemetryEvent::AppStarted { locale: None });
        envelope.event_id = event_id.to_owned();
        envelope.occurred_at = occurred_at;
        StoredEvent { received_at: Utc::now(), envelope }
    }

    #[test]
    fn the_key_is_the_day_the_event_happened() {
        let occurred = Utc::now() - chrono::Duration::days(2);
        let event = event_at(occurred, "11111111-2222-4333-8444-555555555555");
        let key = event_key(DEFAULT_PREFIX, event.occurred_at(), &event.envelope.event_id);

        assert_eq!(
            key,
            format!(
                "raw/events/dt={}/11111111-2222-4333-8444-555555555555.json",
                occurred.format("%Y-%m-%d")
            )
        );
        // The collector's clock is today; a key derived from it would say so.
        assert!(!key.contains(&format!("dt={}", Utc::now().format("%Y-%m-%d"))));
    }

    #[test]
    fn a_crash_is_keyed_by_fingerprint_and_id() {
        let key = crash_key("raw", "abc123", "r-1");
        assert_eq!(key, "raw/crashes/abc123/r-1.json");
    }

    #[test]
    fn an_id_cannot_add_a_path_segment() {
        let occurred = Utc::now();
        let key = event_key("raw", occurred, "../../etc/passwd");
        assert!(!key.contains(".."), "{key}");
        assert_eq!(
            key,
            format!("raw/events/dt={}/etcpasswd.json", occurred.format("%Y-%m-%d"))
        );
    }

    #[test]
    fn the_prefix_is_trimmed_and_optional() {
        assert_eq!(normalise_prefix("/raw/"), "raw");
        assert_eq!(normalise_prefix(""), "");
        assert_eq!(join("", "events/x.json"), "events/x.json");
        assert_eq!(join("raw", "events/x.json"), "raw/events/x.json");
    }

    #[test]
    fn a_stored_event_is_flat_for_athena() {
        let value = serde_json::to_value(event_at(Utc::now(), "id-1")).unwrap();
        let map = value.as_object().unwrap();
        for key in ["schema", "event_id", "occurred_at", "received_at", "context", "event"] {
            assert!(map.contains_key(key), "{key} is missing from {value}");
        }
    }
}
