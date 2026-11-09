//! The live telemetry streams, each a direct 1:1 call to one server-streaming RPC, collected the
//! way `events_subscribe` collects: for a caller-chosen window (default 10 s, capped at 60 s),
//! stopping early at 500 items, and returned in one result.
//! - `telemetry_records` → `TelemetryService.StreamRecords`: the ingest's `sample`, `gap` and
//!   `phase` records from a cursor. `follow` keeps listening for the window; without it the call
//!   ends at the last record written so far. `lastSeq` is the cursor for the next call.
//! - `telemetry_metrics` → `TelemetryService.StreamMetrics`: the samples the ingest writes while
//!   the window is open, optionally filtered by labels.
//! - `logs_follow` → `LogService.Follow`: the log lines the ingest read from each entity's files,
//!   from a cursor, filtered by entity prefix and stream.

use std::collections::HashMap;
use std::time::Duration;

use rmcp::model::CallToolResult;
use tokio_stream::{Stream, StreamExt};
use vxedge_api::v1::log_service_client::LogServiceClient;
use vxedge_api::v1::telemetry_service_client::TelemetryServiceClient;
use vxedge_api::v1::{LogFollowRequest, StreamMetricsRequest, StreamRecordsRequest};

use super::experiment::{path_ref, resolve_workspace};

const DEFAULT_WINDOW_SECS: u64 = 10;
const MAX_WINDOW_SECS: u64 = 60;
/// Keeps one result a readable size for an agent; page with the cursor for more.
const MAX_ITEMS: usize = 500;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RecordsArgs {
    /// Path to the experiment workspace whose telemetry service's ingest to read.
    pub workspace: String,
    /// Only records after this sequence number (the previous result's `lastSeq`). Omitted means
    /// from the first record.
    #[serde(default)]
    pub after: Option<i64>,
    /// Keep listening for new records until the window closes. Omitted means return what has been
    /// written so far.
    #[serde(default)]
    pub follow: bool,
    /// With `follow`: how long to listen, in seconds. Default 10, capped at 60.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MetricsArgs {
    /// Path to the experiment workspace whose metrics to read.
    pub workspace: String,
    /// Only samples whose labels include all of these, e.g. `{"slice": "adas"}`.
    #[serde(default)]
    pub selector: HashMap<String, String>,
    /// How long to listen, in seconds. Default 10, capped at 60.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LogsArgs {
    /// Path to the experiment workspace whose log lines to read.
    pub workspace: String,
    /// Only lines after this sequence number (the previous result's `lastSeq`). Omitted means
    /// from the first line.
    #[serde(default)]
    pub after: Option<i64>,
    /// Only entities whose path starts with this, e.g. `car/front` or `host/services`.
    #[serde(default)]
    pub entity_prefix: Option<String>,
    /// Only these streams, e.g. `serial`, `vmm`, `stdout`. Empty or omitted means every stream.
    #[serde(default)]
    pub streams: Vec<String>,
    /// Keep listening for new lines until the window closes.
    #[serde(default)]
    pub follow: bool,
    /// With `follow`: how long to listen, in seconds. Default 10, capped at 60.
    #[serde(default)]
    pub duration_secs: Option<u64>,
}

fn window(requested: Option<u64>) -> Duration {
    Duration::from_secs(
        requested
            .unwrap_or(DEFAULT_WINDOW_SECS)
            .clamp(1, MAX_WINDOW_SECS),
    )
}

/// How a collection ended, named in the result so an agent knows whether to page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    WindowClosed,
    Capped,
    StreamEnded,
}

impl End {
    fn as_str(self) -> &'static str {
        match self {
            End::WindowClosed => "window_closed",
            End::Capped => "capped",
            End::StreamEnded => "stream_ended",
        }
    }
}

/// Generic over the stream, as `events.rs`'s `collect_events` is, so it is tested without a
/// daemon.
async fn collect<S, T>(
    mut stream: S,
    window: Duration,
    max: usize,
) -> Result<(Vec<T>, End), tonic::Status>
where
    S: Stream<Item = Result<T, tonic::Status>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + window;
    let mut items = Vec::new();
    let end = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Err(_elapsed) => break End::WindowClosed,
            Ok(None) => break End::StreamEnded,
            Ok(Some(Err(status))) => return Err(status),
            Ok(Some(Ok(item))) => {
                items.push(item);
                if items.len() >= max {
                    break End::Capped;
                }
            }
        }
    };
    Ok((items, end))
}

fn result<T: serde::Serialize>(
    key: &str,
    items: &[T],
    end: End,
    last_seq: Option<i64>,
) -> anyhow::Result<CallToolResult> {
    let mut out = serde_json::json!({ key: items, "count": items.len(), "end": end.as_str() });
    if let Some(seq) = last_seq {
        out["lastSeq"] = seq.into();
    }
    super::out::json(&out)
}

pub async fn records(
    channel: tonic::transport::Channel,
    args: RecordsArgs,
) -> anyhow::Result<CallToolResult> {
    let stream = TelemetryServiceClient::new(channel)
        .stream_records(StreamRecordsRequest {
            experiment: path_ref(resolve_workspace(&args.workspace)),
            after: args.after.unwrap_or(0),
            follow: args.follow,
            ..Default::default()
        })
        .await?
        .into_inner();
    let (records, end) = collect(stream, window(args.duration_secs), MAX_ITEMS).await?;
    let last = records.last().map(|r| r.seq).or(args.after);
    result("records", &records, end, last)
}

pub async fn metrics(
    channel: tonic::transport::Channel,
    args: MetricsArgs,
) -> anyhow::Result<CallToolResult> {
    let stream = TelemetryServiceClient::new(channel)
        .stream_metrics(StreamMetricsRequest {
            experiment: path_ref(resolve_workspace(&args.workspace)),
            selector: args.selector,
        })
        .await?
        .into_inner();
    let (samples, end) = collect(stream, window(args.duration_secs), MAX_ITEMS).await?;
    result("samples", &samples, end, None)
}

pub async fn logs(
    channel: tonic::transport::Channel,
    args: LogsArgs,
) -> anyhow::Result<CallToolResult> {
    let stream = LogServiceClient::new(channel)
        .follow(LogFollowRequest {
            experiment: path_ref(resolve_workspace(&args.workspace)),
            after: args.after.unwrap_or(0),
            follow: args.follow,
            entity_prefix: args.entity_prefix.unwrap_or_default(),
            streams: args.streams,
        })
        .await?
        .into_inner();
    let (lines, end) = collect(stream, window(args.duration_secs), MAX_ITEMS).await?;
    let last = lines.last().map(|l| l.seq).or(args.after);
    result("lines", &lines, end, last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_need_only_a_workspace() {
        let r: RecordsArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/w" })).unwrap();
        assert!(r.after.is_none() && !r.follow && r.duration_secs.is_none());
        let l: LogsArgs = serde_json::from_value(serde_json::json!({ "workspace": "/w" })).unwrap();
        assert!(l.streams.is_empty() && l.entity_prefix.is_none());
        let m: MetricsArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/w" })).unwrap();
        assert!(m.selector.is_empty());
    }

    #[test]
    fn the_window_defaults_to_ten_seconds_and_is_capped() {
        assert_eq!(window(None), Duration::from_secs(10));
        assert_eq!(window(Some(0)), Duration::from_secs(1));
        assert_eq!(window(Some(600)), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn collection_stops_at_the_cap_the_end_or_the_window() {
        let items = (1..=5).map(Ok::<_, tonic::Status>);
        let (got, end) = collect(tokio_stream::iter(items), Duration::from_secs(1), 3)
            .await
            .unwrap();
        assert_eq!((got, end), (vec![1, 2, 3], End::Capped));
        let (got, end) = collect(
            tokio_stream::iter([Ok::<_, tonic::Status>(7)]),
            Duration::from_secs(1),
            3,
        )
        .await
        .unwrap();
        assert_eq!((got, end), (vec![7], End::StreamEnded));
        let pending = tokio_stream::pending::<Result<i32, tonic::Status>>();
        let (got, end) = collect(pending, Duration::from_millis(20), 3)
            .await
            .unwrap();
        assert_eq!((got, end), (vec![], End::WindowClosed));
    }

    #[tokio::test]
    async fn an_error_in_the_stream_is_returned() {
        let items = [Ok(1), Err(tonic::Status::failed_precondition("no ingest"))];
        let err = collect(tokio_stream::iter(items), Duration::from_secs(1), 10)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }
}
