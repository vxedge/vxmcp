//! `JobService` tools. Every mutating RPC returns a `Job`; these are how an agent checks on one.
//! - `job_get` — a direct 1:1 call to `JobService.Get`: one snapshot by id.
//! - `job_watch` — a direct 1:1 call to `JobService.Watch` (server streaming). An MCP tool call
//!   returns once, so the stream is followed until the job reaches a terminal state or a
//!   caller-chosen timeout (default 60s, capped at 300s) passes, and the distinct steps seen
//!   are returned in one result (`TASKS.md` #2: collect-until-terminal-or-timeout, chosen
//!   2026-10-29). Nothing is retried or sequenced here — on a timeout the agent decides
//!   whether to watch again.

use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock};
use tokio_stream::{Stream, StreamExt};
use vxedge_api::v1::job_service_client::JobServiceClient;
use vxedge_api::v1::{GetJobRequest, Job, JobState, WatchJobRequest};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct JobGetArgs {
    /// The job id, as returned in the `Job` from any mutating vxAPI call.
    pub id: String,
}

pub async fn get(
    channel: tonic::transport::Channel,
    args: JobGetArgs,
) -> anyhow::Result<CallToolResult> {
    let mut client = JobServiceClient::new(channel);
    let resp = client
        .get(GetJobRequest { id: args.id })
        .await?
        .into_inner();
    Ok(CallToolResult::success(vec![ContentBlock::text(render(
        &resp,
    ))]))
}

/// Named job states, mirroring vxedge-cli's own `state_name()` — job states are the one place
/// the CLI already prints names rather than ordinals, so this follows it.
fn state_name(state: i32) -> &'static str {
    match JobState::try_from(state) {
        Ok(JobState::Pending) => "pending",
        Ok(JobState::Running) => "running",
        Ok(JobState::Succeeded) => "succeeded",
        Ok(JobState::Failed) => "failed",
        Ok(JobState::Cancelled) => "cancelled",
        _ => "unknown",
    }
}

fn render(job: &Job) -> String {
    let mut out = format!(
        "{} {}: {} [{}%] {}",
        job.id,
        job.kind,
        state_name(job.state),
        job.progress_percent,
        job.message
    )
    .trim_end()
    .to_string();
    if !job.error.is_empty() {
        out.push_str(&format!("\n  error: {}", job.error));
    }
    out
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct JobWatchArgs {
    /// The job id, as returned in the `Job` from any mutating vxAPI call.
    pub id: String,
    /// How long to follow the job before returning, in seconds. Default 60, capped at 300.
    /// On a timeout the result says the job is still running; call again to keep watching.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

const DEFAULT_WATCH_SECS: u64 = 60;
const MAX_WATCH_SECS: u64 = 300;

fn watch_secs(requested: Option<u64>) -> u64 {
    requested
        .unwrap_or(DEFAULT_WATCH_SECS)
        .clamp(1, MAX_WATCH_SECS)
}

pub async fn watch(
    channel: tonic::transport::Channel,
    args: JobWatchArgs,
) -> anyhow::Result<CallToolResult> {
    let secs = watch_secs(args.timeout_secs);
    let mut client = JobServiceClient::new(channel);
    let stream = client
        .watch(WatchJobRequest { id: args.id })
        .await?
        .into_inner();
    let outcome = collect_job(stream, Duration::from_secs(secs)).await?;
    Ok(CallToolResult::success(vec![ContentBlock::text(
        render_watch(&outcome, secs),
    )]))
}

#[derive(Debug)]
enum WatchEnd {
    Terminal,
    TimedOut,
    StreamEnded,
}

#[derive(Debug)]
struct WatchOutcome {
    /// Snapshots whose (progress, message) differ from the one before — the steps a human
    /// would see in `vxedge`'s own progress output.
    steps: Vec<Job>,
    last: Option<Job>,
    end: WatchEnd,
}

fn is_terminal(state: i32) -> bool {
    matches!(
        JobState::try_from(state),
        Ok(JobState::Succeeded | JobState::Failed | JobState::Cancelled)
    )
}

/// Generic over the stream so the terminal/timeout/early-end paths are unit-testable without a
/// daemon; `tonic::Streaming<Job>` is the only real caller.
async fn collect_job<S>(mut stream: S, limit: Duration) -> Result<WatchOutcome, tonic::Status>
where
    S: Stream<Item = Result<Job, tonic::Status>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + limit;
    let mut steps: Vec<Job> = Vec::new();
    let mut last: Option<Job> = None;
    let end = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Err(_elapsed) => break WatchEnd::TimedOut,
            Ok(None) => break WatchEnd::StreamEnded,
            Ok(Some(Err(status))) => return Err(status),
            Ok(Some(Ok(job))) => {
                // Watch sends the current snapshot on subscribe and again on the first change,
                // so compare on what is shown, not the whole job (`updated_unix_ms` always
                // differs) — the same dedupe vxedge-cli's own watch_job() does.
                let shown = (job.progress_percent, job.message.as_str());
                let repeat = steps
                    .last()
                    .is_some_and(|p| (p.progress_percent, p.message.as_str()) == shown);
                if !repeat {
                    steps.push(job.clone());
                }
                let terminal = is_terminal(job.state);
                last = Some(job);
                if terminal {
                    break WatchEnd::Terminal;
                }
            }
        }
    };
    Ok(WatchOutcome { steps, last, end })
}

fn render_watch(outcome: &WatchOutcome, secs: u64) -> String {
    let Some(last) = &outcome.last else {
        return format!("no job updates received within {secs}s");
    };
    let mut lines: Vec<String> = outcome
        .steps
        .iter()
        // Same filter vxedge-cli applies: a snapshot with no message and no progress is not
        // a step worth showing.
        .filter(|j| !j.message.is_empty() || j.progress_percent > 0)
        .map(|j| format!("[{:>3}%] {} {}", j.progress_percent, j.kind, j.message))
        .collect();
    match outcome.end {
        WatchEnd::Terminal => {}
        WatchEnd::TimedOut => lines.push(format!(
            "still {} after {secs}s — call job_watch again to keep following it",
            state_name(last.state)
        )),
        WatchEnd::StreamEnded => {
            lines.push("stream ended before the job reached a terminal state".to_string())
        }
    }
    lines.push(render(last));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vxedge_api::v1::{Job, JobState};

    #[test]
    fn args_deserialize_from_a_bare_id_field() {
        let args: JobGetArgs = serde_json::from_value(serde_json::json!({ "id": "j-42" })).unwrap();
        assert_eq!(args.id, "j-42");
    }

    #[test]
    fn renders_a_running_job_with_named_state_and_progress() {
        let job = Job {
            id: "j-42".into(),
            kind: "gateway.up".into(),
            state: JobState::Running as i32,
            progress_percent: 40,
            message: "configuring bridge".into(),
            ..Default::default()
        };
        assert_eq!(
            render(&job),
            "j-42 gateway.up: running [40%] configuring bridge"
        );
    }

    #[test]
    fn appends_the_error_line_for_a_failed_job() {
        let job = Job {
            id: "j-7".into(),
            kind: "zone.up".into(),
            state: JobState::Failed as i32,
            progress_percent: 60,
            message: "booting".into(),
            error: "qemu exited 1".into(),
            ..Default::default()
        };
        assert_eq!(
            render(&job),
            "j-7 zone.up: failed [60%] booting\n  error: qemu exited 1"
        );
    }

    #[test]
    fn unknown_state_ordinals_render_as_unknown() {
        let job = Job {
            id: "j-9".into(),
            kind: "x".into(),
            state: 99,
            ..Default::default()
        };
        assert_eq!(render(&job), "j-9 x: unknown [0%]");
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;
    use std::time::Duration;
    use vxedge_api::v1::{Job, JobState};

    fn snap(state: JobState, pct: u32, msg: &str) -> Job {
        Job {
            id: "j-1".into(),
            kind: "gateway.up".into(),
            state: state as i32,
            progress_percent: pct,
            message: msg.into(),
            ..Default::default()
        }
    }

    #[test]
    fn watch_args_default_and_cap_the_timeout() {
        let a: JobWatchArgs = serde_json::from_value(serde_json::json!({ "id": "j-1" })).unwrap();
        assert_eq!(a.id, "j-1");
        assert_eq!(watch_secs(a.timeout_secs), 60);
        assert_eq!(watch_secs(Some(9_999)), 300);
        assert_eq!(watch_secs(Some(0)), 1);
    }

    #[tokio::test]
    async fn stops_at_a_terminal_state_and_dedupes_repeated_steps() {
        // Watch delivers the current snapshot on subscribe and again on first change, so the
        // same step arrives twice — vxedge-cli's own watch_job() hides the repeat too.
        let s = tokio_stream::iter(vec![
            Ok(snap(JobState::Running, 10, "bridge")),
            Ok(snap(JobState::Running, 10, "bridge")),
            Ok(snap(JobState::Running, 60, "vlans")),
            Ok(snap(JobState::Succeeded, 100, "done")),
            Ok(snap(JobState::Succeeded, 100, "ignored after terminal")),
        ]);
        let out = collect_job(s, Duration::from_secs(5)).await.unwrap();
        assert!(matches!(out.end, WatchEnd::Terminal));
        assert_eq!(
            render_watch(&out, 5),
            "[ 10%] gateway.up bridge\n[ 60%] gateway.up vlans\n[100%] gateway.up done\n\
             j-1 gateway.up: succeeded [100%] done"
        );
    }

    #[tokio::test]
    async fn times_out_on_a_job_that_is_still_running() {
        let s = tokio_stream::iter(vec![Ok(snap(JobState::Running, 40, "booting"))])
            .chain(tokio_stream::pending());
        let out = collect_job(s, Duration::from_millis(30)).await.unwrap();
        assert!(matches!(out.end, WatchEnd::TimedOut));
        let text = render_watch(&out, 7);
        assert!(text.starts_with("[ 40%] gateway.up booting\n"));
        assert!(text.contains("still running after 7s"));
        assert!(text.ends_with("j-1 gateway.up: running [40%] booting"));
    }

    #[tokio::test]
    async fn reports_a_stream_that_ends_before_a_terminal_state() {
        let s = tokio_stream::iter(vec![Ok(snap(JobState::Running, 0, ""))]);
        let out = collect_job(s, Duration::from_secs(5)).await.unwrap();
        assert!(matches!(out.end, WatchEnd::StreamEnded));
        assert!(
            render_watch(&out, 5).contains("stream ended before the job reached a terminal state")
        );
    }

    #[tokio::test]
    async fn says_so_when_nothing_arrives_in_time() {
        let out = collect_job(tokio_stream::pending(), Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(render_watch(&out, 3), "no job updates received within 3s");
    }

    #[tokio::test]
    async fn propagates_a_stream_error() {
        let s = tokio_stream::iter(vec![Err(tonic::Status::not_found("unknown job `x`"))]);
        let err = collect_job(s, Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }
}
