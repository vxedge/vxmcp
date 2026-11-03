//! `telemetry_collect` — a direct 1:1 call to `TelemetryService.Collect`, behind
//! `crate::confirm` because it writes a file on the host. vxedged runs `vxtelemetry collect` as
//! a job over what the workspace's telemetry service ingested, records the digest of every
//! image the workspace runs, and returns the archive's path and sha256 as `Job.artifact`.
//! `StreamMetrics` is not built, so it has no tool.

use rmcp::model::CallToolResult;
use vxedge_api::v1::CollectRequest;
use vxedge_api::v1::telemetry_service_client::TelemetryServiceClient;

use super::experiment::path_ref;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CollectArgs {
    /// Path to the experiment workspace whose results to collect. You will be asked to confirm.
    pub workspace: String,
    /// Where to write the archive. Relative paths are resolved against the workspace. Omitted
    /// means `<workspace>/results/<experiment id>-<unix ms>.zip`.
    #[serde(default)]
    pub out_path: Option<String>,
}

/// The question the human sees before `telemetry_collect` runs, for an already-resolved path.
/// Checked against vxedged's `TelemetryService.Collect`.
pub fn collect_confirmation(resolved_workspace: &str, out_path: Option<&str>) -> String {
    let dest = match out_path {
        Some(p) if p.starts_with('/') => format!("`{p}`"),
        Some(p) if !p.is_empty() => format!("`{resolved_workspace}/{p}`"),
        _ => format!("`{resolved_workspace}/results/`"),
    };
    format!(
        "Collect the telemetry `{resolved_workspace}` ingested into an archive at {dest}, with \
         the digest of every image it runs. This writes a file on this host. This affects \
         anyone using this host's testbed. Confirm?"
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn collect(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    out_path: Option<String>,
) -> anyhow::Result<CallToolResult> {
    let job = TelemetryServiceClient::new(channel)
        .collect(CollectRequest {
            manifest: path_ref(resolved_workspace),
            out_path: out_path.unwrap_or_default(),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_names_the_destination() {
        assert!(collect_confirmation("/ws/e", None).contains("archive at `/ws/e/results/`"));
        assert!(collect_confirmation("/ws/e", Some("/tmp/r.zip")).contains("at `/tmp/r.zip`"));
        assert!(collect_confirmation("/ws/e", Some("r.zip")).contains("at `/ws/e/r.zip`"));
    }
}
