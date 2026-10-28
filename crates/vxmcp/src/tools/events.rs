//! `events_subscribe` — a direct 1:1 call to `EventService.Subscribe` (server streaming).
//! The daemon's bus is live-only (no history: `vxedged`'s `BroadcastStream` over its event
//! bus), so this collects whatever is emitted during a caller-chosen window (default 10s,
//! capped at 60s), stopping early at 200 events, and returns them in one result — the same
//! collect-then-return shape as `job_watch` (`TASKS.md` #2, chosen 2026-10-29).

use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock};
use tokio_stream::{Stream, StreamExt};
use vxedge_api::v1::event_service_client::EventServiceClient;
use vxedge_api::v1::{Event, SubscribeRequest};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EventsSubscribeArgs {
    /// Event kinds to keep, e.g. `state-transition`, `audit`, `reconcile`. Empty or omitted
    /// means every kind; the daemon does the filtering.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// How long to listen, in seconds. Default 10, capped at 60. Only events emitted during
    /// this window are returned — the daemon keeps no event history.
    #[serde(default)]
    pub duration_secs: Option<u64>,
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
    let mut client = EventServiceClient::new(channel);
    let stream = client
        .subscribe(SubscribeRequest { kinds: args.kinds })
        .await?
        .into_inner();
    let (events, end) = collect_events(stream, Duration::from_secs(secs), MAX_EVENTS).await?;
    Ok(CallToolResult::success(vec![ContentBlock::text(
        render_events(&events, &end, secs),
    )]))
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

fn render_events(events: &[Event], end: &EventsEnd, secs: u64) -> String {
    if events.is_empty() && matches!(end, EventsEnd::WindowClosed) {
        return format!("no events in {secs}s");
    }
    // `resource_kind` stays a bare ResourceKind ordinal, matching the crate's `[state=N]`
    // convention for enums.
    let mut lines: Vec<String> = events
        .iter()
        .map(|e| {
            format!(
                "{} {} {} (resource_kind={}) {}",
                e.ts_unix_ms, e.kind, e.resource_vxid, e.resource_kind, e.detail
            )
        })
        .collect();
    match end {
        EventsEnd::WindowClosed => {}
        EventsEnd::Capped => lines.push(format!("(stopped at {} events)", events.len())),
        EventsEnd::StreamEnded => lines.push("(event stream ended early)".to_string()),
    }
    lines.join("\n")
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
    }

    #[tokio::test]
    async fn collects_until_the_window_closes() {
        let s = tokio_stream::iter(vec![Ok(ev(1, "audit")), Ok(ev(2, "state-transition"))])
            .chain(tokio_stream::pending());
        let (events, end) = collect_events(s, Duration::from_millis(30), 200)
            .await
            .unwrap();
        assert!(matches!(end, EventsEnd::WindowClosed));
        assert_eq!(
            render_events(&events, &end, 4),
            "1 audit zone-front (resource_kind=2) up\n2 state-transition zone-front (resource_kind=2) up"
        );
    }

    #[tokio::test]
    async fn stops_at_the_event_cap_and_says_so() {
        let s =
            tokio_stream::iter((1..=5).map(|n| Ok(ev(n, "audit")))).chain(tokio_stream::pending());
        let (events, end) = collect_events(s, Duration::from_secs(5), 3).await.unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(end, EventsEnd::Capped));
        assert!(render_events(&events, &end, 5).ends_with("(stopped at 3 events)"));
    }

    #[tokio::test]
    async fn says_so_when_no_events_arrive() {
        let (events, end) = collect_events(tokio_stream::pending(), Duration::from_millis(10), 200)
            .await
            .unwrap();
        assert_eq!(render_events(&events, &end, 2), "no events in 2s");
    }

    #[tokio::test]
    async fn reports_a_stream_that_ends_early() {
        let s = tokio_stream::iter(vec![Ok(ev(1, "audit"))]);
        let (events, end) = collect_events(s, Duration::from_secs(5), 200)
            .await
            .unwrap();
        assert!(matches!(end, EventsEnd::StreamEnded));
        assert!(render_events(&events, &end, 5).ends_with("(event stream ended early)"));
    }
}
