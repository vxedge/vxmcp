//! `job_get` — a direct 1:1 call to `JobService.Get`: one snapshot of a job by id. Every
//! mutating RPC returns a `Job`; this is how an agent checks on one without the streaming
//! `JobService.Watch` (which needs its own design decision first, `TASKS.md` #2).

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::job_service_client::JobServiceClient;
use vxedge_api::v1::{GetJobRequest, Job, JobState};

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
