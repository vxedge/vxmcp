//! `events_subscribe` — a direct 1:1 call to `EventService.Subscribe` (server streaming). It
//! collects what is emitted during a caller-chosen window (default 10s, capped at 60s), stopping
//! early at 200 events, and returns them in one result — the same collect-then-return shape as
//! `job_watch`. vxedged keeps a bounded history (1,024 events), so `after_id`
//! replays from a cursor with no gap; `lastId` in the result is the cursor for the next call. A
//! cursor the history no longer holds is refused as `ERROR_REASON_EVENTS_LOST`: re-read status,
//! then subscribe again.

use std::time::Duration;

use rmcp::model::CallToolResult;
use tokio_stream::{Stream, StreamExt};
use vxedge_api::v1::event_service_client::EventServiceClient;
use vxedge_api::v1::{Event, SubscribeRequest};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EventsSubscribeArgs {
    /// Event kinds to keep, e.g. `run.up` or a family such as `zone` (matching `zone.up` and
    /// `zone.down`). Empty or omitted means every kind; the daemon does the filtering.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// How long to listen, in seconds. Default 10, capped at 60.
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// Resume after this event id (the previous result's `lastId`): history after it is replayed
    /// first. Omitted means live events only.
    #[serde(default)]
    pub after_id: Option<String>,
}

const DEFAULT_WINDOW_SECS: u64 = 10;
const MAX_WINDOW_SECS: u64 = 60;
/// Keeps one result a readable size for an agent; the window rarely fills this.
const MAX_EVENTS: usize = 200;

fn window_secs(requested: Option<u64>) -> u64 {
    requested
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(1, MAX_WINDOW_SECS)
}

pub async fn subscribe(
    channel: tonic::transport::Channel,
    args: EventsSubscribeArgs,
) -> anyhow::Result<CallToolResult> {
    let secs = window_secs(args.duration_secs);
    let stream = EventServiceClient::new(channel)
        .subscribe(SubscribeRequest {
            kinds: args.kinds,
            after_id: args.after_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    let (events, end) = collect_events(stream, Duration::from_secs(secs), MAX_EVENTS).await?;
    super::out::json(&events_json(&events, &end, secs))
}

#[derive(Debug)]
enum EventsEnd {
    WindowClosed,
    Capped,
    StreamEnded,
}

/// Generic over the stream for the same reason as `job.rs`'s `collect_job`.
async fn collect_events<S>(
    mut stream: S,
    window: Duration,
    max: usize,
) -> Result<(Vec<Event>, EventsEnd), tonic::Status>
where
    S: Stream<Item = Result<Event, tonic::Status>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + window;
    let mut events = Vec::new();
    let end = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Err(_elapsed) => break EventsEnd::WindowClosed,
            Ok(None) => break EventsEnd::StreamEnded,
            Ok(Some(Err(status))) => return Err(status),
            Ok(Some(Ok(event))) => {
                events.push(event);
                if events.len() >= max {
                    break EventsEnd::Capped;
                }
            }
        }
    };
    Ok((events, end))
}

/// `end` says why collecting stopped: `window_closed`, `capped` at the event limit, or
/// `stream_ended`. `lastId` is the cursor for a follow-up call's `after_id`; with no events it
/// is absent, and the caller keeps its previous cursor.
fn events_json(events: &[Event], end: &EventsEnd, secs: u64) -> serde_json::Value {
    let end = match end {
        EventsEnd::WindowClosed => "window_closed",
        EventsEnd::Capped => "capped",
        EventsEnd::StreamEnded => "stream_ended",
    };
    let mut v = serde_json::json!({ "end": end, "windowSecs": secs, "events": events });
    if let Some(last) = events.last() {
        v["lastId"] = serde_json::Value::String(last.id.clone());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use vxedge_api::v1::Event;

    fn ev(n: i64, kind: &str) -> Event {
        Event {
            id: format!("e-{n}"),
            ts_unix_ms: n,
            kind: kind.into(),
            resource_kind: 2,
            resource_vxid: "zone-front".into(),
            detail: "up".into(),
            run_id: "run-1".into(),
        }
    }

    #[test]
    fn args_default_and_cap_the_window() {
        let a: EventsSubscribeArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(a.kinds.is_empty());
        assert_eq!(window_secs(a.duration_secs), 10);
        assert_eq!(window_secs(Some(9_999)), 60);
        assert_eq!(window_secs(Some(0)), 1);
        let b: EventsSubscribeArgs =
            serde_json::from_value(serde_json::json!({ "kinds": ["audit"], "duration_secs": 5 }))
                .unwrap();
        assert_eq!(b.kinds, vec!["audit".to_string()]);
        assert_eq!(b.after_id, None);
    }

    #[tokio::test]
    async fn collects_until_the_window_closes() {
        let s = tokio_stream::iter(vec![Ok(ev(1, "audit")), Ok(ev(2, "state-transition"))])
            .chain(tokio_stream::pending());
        let (events, end) = collect_events(s, Duration::from_millis(30), 200)
            .await
            .unwrap();
        assert!(matches!(end, EventsEnd::WindowClosed));
        let v = events_json(&events, &end, 4);
        assert_eq!(v["end"], "window_closed");
        assert_eq!(v["events"][1]["kind"], "state-transition");
        assert_eq!(v["events"][0]["runId"], "run-1");
        assert_eq!(v["lastId"], "e-2");
    }

    #[tokio::test]
    async fn stops_at_the_event_cap_and_says_so() {
        let s =
            tokio_stream::iter((1..=5).map(|n| Ok(ev(n, "audit")))).chain(tokio_stream::pending());
        let (events, end) = collect_events(s, Duration::from_secs(5), 3).await.unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(end, EventsEnd::Capped));
        assert_eq!(events_json(&events, &end, 5)["end"], "capped");
    }

    #[tokio::test]
    async fn says_so_when_no_events_arrive() {
        let (events, end) = collect_events(tokio_stream::pending(), Duration::from_millis(10), 200)
            .await
            .unwrap();
        let v = events_json(&events, &end, 2);
        assert_eq!(v["events"].as_array().unwrap().len(), 0);
        assert!(v.get("lastId").is_none());
    }

    #[tokio::test]
    async fn reports_a_stream_that_ends_early() {
        let s = tokio_stream::iter(vec![Ok(ev(1, "audit"))]);
        let (events, end) = collect_events(s, Duration::from_secs(5), 200)
            .await
            .unwrap();
        assert!(matches!(end, EventsEnd::StreamEnded));
        assert_eq!(events_json(&events, &end, 5)["end"], "stream_ended");
    }
}
