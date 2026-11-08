//! `JobService` tools. Every mutating RPC returns a `Job`; these are how an agent checks on one.
//! - `job_get` → `Get`: one snapshot by id.
//! - `job_list` → `List`: jobs filtered by state, kind, run or parent.
//! - `job_cancel` → `Cancel`, behind `crate::confirm`: cooperative, so the job stops between
//!   steps; its child jobs are cancelled too, and a `down` is refused.
//! - `job_watch` → `Watch` (server streaming). An MCP tool call returns once, so the stream is
//!   followed until the job reaches a terminal state or a caller-chosen timeout (default 60s,
//!   capped at 300s) passes, and the distinct steps seen are returned in one result. Nothing is retried or sequenced here — on a timeout the agent decides whether to
//!   watch again.

use std::time::Duration;

use rmcp::model::CallToolResult;
use tokio_stream::{Stream, StreamExt};
use vxedge_api::v1::job_service_client::JobServiceClient;
use vxedge_api::v1::{
    GetJobRequest, Job, JobCancelRequest, JobListRequest, JobState, WatchJobRequest,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct JobGetArgs {
    /// The job id, as returned in the `Job` from any mutating vxAPI call.
    pub id: String,
}

pub async fn get(
    channel: tonic::transport::Channel,
    args: JobGetArgs,
) -> anyhow::Result<CallToolResult> {
    let job = JobServiceClient::new(channel)
        .get(GetJobRequest { id: args.id })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct JobListArgs {
    /// Keep only jobs in these states: `pending`, `running`, `succeeded`, `failed`,
    /// `cancelled`. Omitted or empty means every state.
    #[serde(default)]
    pub states: Vec<String>,
    /// Keep only jobs of this kind, e.g. `experiment.up`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Keep only jobs of this run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Keep only the child jobs of this job.
    #[serde(default)]
    pub parent_job_id: Option<String>,
    /// Page size; vxedged applies its own default and cap.
    #[serde(default)]
    pub page_size: Option<u32>,
    /// `nextPageToken` from the previous page.
    #[serde(default)]
    pub page_token: Option<String>,
}

fn job_states(names: &[String]) -> anyhow::Result<Vec<i32>> {
    names
        .iter()
        .map(
            |n| match JobState::from_str_name(&format!("JOB_STATE_{}", n.to_ascii_uppercase())) {
                Some(JobState::Unspecified) | None => Err(super::out::ArgError(format!(
                    "unknown job state `{n}`: use pending, running, succeeded, failed or cancelled"
                ))
                .into()),
                Some(s) => Ok(s as i32),
            },
        )
        .collect()
}

pub async fn list(
    channel: tonic::transport::Channel,
    args: JobListArgs,
) -> anyhow::Result<CallToolResult> {
    let states = job_states(&args.states)?;
    let resp = JobServiceClient::new(channel)
        .list(JobListRequest {
            states,
            kind: args.kind.unwrap_or_default(),
            run: super::out::run_ref(args.run_id),
            parent_job_id: args.parent_job_id.unwrap_or_default(),
            page_size: args.page_size.unwrap_or_default(),
            page_token: args.page_token.unwrap_or_default(),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct JobCancelArgs {
    /// The job id to cancel. You will be asked to confirm.
    pub id: String,
}

/// The question the human sees before `job_cancel` runs. Checked against vxedged's
/// `JobEngine::cancel`: cooperative, cascades to child jobs, refuses a `down`.
pub fn cancel_confirmation(id: &str) -> String {
    format!(
        "Cancel job `{id}`: it stops at its next step and its child jobs are cancelled too, \
         which can leave a run partly up until `down` runs. vxedged refuses to cancel a `down`. \
         This affects anyone using this host's testbed. Confirm?"
    )
}

pub async fn cancel(
    channel: tonic::transport::Channel,
    args: JobCancelArgs,
) -> anyhow::Result<CallToolResult> {
    let job = JobServiceClient::new(channel)
        .cancel(JobCancelRequest { id: args.id })
        .await?
        .into_inner();
    super::out::json(&job)
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
    let stream = JobServiceClient::new(channel)
        .watch(WatchJobRequest { id: args.id })
        .await?
        .into_inner();
    let outcome = collect_job(stream, Duration::from_secs(secs)).await?;
    super::out::json(&watch_json(&outcome, secs))
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

/// `end` says why watching stopped: `terminal`, `timed_out` (call again to keep following),
/// `stream_ended` before a terminal state, or `no_updates` within the timeout. `job` is the
/// last snapshot; `steps` the distinct progress steps seen, as vxedge-cli prints them.
fn watch_json(outcome: &WatchOutcome, secs: u64) -> serde_json::Value {
    let end = match (&outcome.last, &outcome.end) {
        (None, _) => "no_updates",
        (_, WatchEnd::Terminal) => "terminal",
        (_, WatchEnd::TimedOut) => "timed_out",
        (_, WatchEnd::StreamEnded) => "stream_ended",
    };
    let steps: Vec<serde_json::Value> = outcome
        .steps
        .iter()
        // Same filter vxedge-cli applies: a snapshot with no message and no progress is not
        // a step worth showing.
        .filter(|j| !j.message.is_empty() || j.progress_percent > 0)
        .map(|j| serde_json::json!({ "progressPercent": j.progress_percent, "message": j.message }))
        .collect();
    serde_json::json!({
        "end": end,
        "timeoutSecs": secs,
        "steps": steps,
        "job": outcome.last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_deserialize_from_a_bare_id_field() {
        let args: JobGetArgs = serde_json::from_value(serde_json::json!({ "id": "j-42" })).unwrap();
        assert_eq!(args.id, "j-42");
    }

    #[test]
    fn list_args_are_all_optional_and_states_map_by_name() {
        let a: JobListArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(a.states.is_empty());
        assert_eq!(
            job_states(&["running".into(), "FAILED".into()]).unwrap(),
            vec![JobState::Running as i32, JobState::Failed as i32]
        );
        assert!(job_states(&["unspecified".into()]).is_err());
        assert!(job_states(&["done".into()]).is_err());
    }

    #[test]
    fn cancel_confirmation_names_the_job_and_ends_with_the_shared_host_line() {
        let t = cancel_confirmation("job-1-7");
        assert!(t.starts_with("Cancel job `job-1-7`"));
        assert!(t.ends_with("This affects anyone using this host's testbed. Confirm?"));
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
        let v = watch_json(&out, 5);
        assert_eq!(v["end"], "terminal");
        assert_eq!(v["job"]["state"], "JOB_STATE_SUCCEEDED");
        let msgs: Vec<&str> = v["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["message"].as_str().unwrap())
            .collect();
        assert_eq!(msgs, ["bridge", "vlans", "done"]);
    }

    #[tokio::test]
    async fn times_out_on_a_job_that_is_still_running() {
        let s = tokio_stream::iter(vec![Ok(snap(JobState::Running, 40, "booting"))])
            .chain(tokio_stream::pending());
        let out = collect_job(s, Duration::from_millis(30)).await.unwrap();
        assert!(matches!(out.end, WatchEnd::TimedOut));
        let v = watch_json(&out, 7);
        assert_eq!(v["end"], "timed_out");
        assert_eq!(v["timeoutSecs"], 7);
        assert_eq!(v["job"]["state"], "JOB_STATE_RUNNING");
        assert_eq!(v["steps"][0]["message"], "booting");
    }

    #[tokio::test]
    async fn reports_a_stream_that_ends_before_a_terminal_state() {
        let s = tokio_stream::iter(vec![Ok(snap(JobState::Running, 0, ""))]);
        let out = collect_job(s, Duration::from_secs(5)).await.unwrap();
        assert!(matches!(out.end, WatchEnd::StreamEnded));
        assert_eq!(watch_json(&out, 5)["end"], "stream_ended");
    }

    #[tokio::test]
    async fn says_so_when_nothing_arrives_in_time() {
        let out = collect_job(tokio_stream::pending(), Duration::from_millis(10))
            .await
            .unwrap();
        let v = watch_json(&out, 3);
        assert_eq!(v["end"], "no_updates");
        assert!(v["job"].is_null());
    }

    #[tokio::test]
    async fn propagates_a_stream_error() {
        let s = tokio_stream::iter(vec![Err(tonic::Status::not_found("unknown job `x`"))]);
        let err = collect_job(s, Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }
}
