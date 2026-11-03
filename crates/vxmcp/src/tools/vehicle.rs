//! `VehicleService` tools, each one RPC. A vehicle workspace names its parts; vxedged plans
//! them onto zone and compute nodes.
//! - `vehicle_status` → `Status`: each stage and node, observed on the host.
//! - `vehicle_up` / `vehicle_down` → `Up` / `Down`, behind `crate::confirm`. Up runs the same
//!   preflight as `workspace_plan` first. Down refuses unless the vehicle is the one active on
//!   this host, and tears stages down in reverse order, best effort.

use rmcp::model::CallToolResult;
use vxedge_api::v1::vehicle_service_client::VehicleServiceClient;
use vxedge_api::v1::{VehicleDownRequest, VehicleStatusRequest, VehicleUpRequest};

use super::experiment::{path_ref, resolve_workspace, stage_words};
pub use super::workspace::WorkspaceArgs as VehicleStatusArgs;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct VehicleStageArgs {
    /// Path to the vehicle workspace — the directory containing its `.vxp` file. You will be
    /// asked to confirm.
    pub workspace: String,
    /// Stages, as `vxedge vehicle up --stage` spells them: `gateway`, `zones`, `services`,
    /// `compute`, `signal-plane`. Omitted or empty means every stage.
    #[serde(default)]
    pub stages: Vec<String>,
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: VehicleStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = VehicleServiceClient::new(channel)
        .status(VehicleStatusRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

/// The question the human sees before `vehicle_up` runs, for an already-resolved path.
pub fn up_confirmation(resolved_workspace: &str, stages: &[String]) -> String {
    format!(
        "Bring up the vehicle at `{resolved_workspace}` ({}): provisions this host's gateway and \
         the zone and compute nodes its parts are planned onto. vxedged checks the host first and \
         refuses if it can't run it or another job holds it. This affects anyone using this \
         host's testbed. Confirm?",
        stage_words(stages)
    )
}

/// The question the human sees before `vehicle_down` runs, for an already-resolved path.
pub fn down_confirmation(resolved_workspace: &str, stages: &[String]) -> String {
    format!(
        "Tear down the vehicle at `{resolved_workspace}` ({}): stops its services, signal plane, \
         compute and zone nodes and this host's gateway, in reverse order. vxedged refuses if it \
         isn't the vehicle active on this host. This affects anyone using this host's testbed. \
         Confirm?",
        stage_words(stages)
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn up(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    stages: &[String],
) -> anyhow::Result<CallToolResult> {
    let stages = super::out::stages(stages)?;
    let job = VehicleServiceClient::new(channel)
        .up(VehicleUpRequest {
            manifest: path_ref(resolved_workspace),
            stages,
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn down(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    stages: &[String],
) -> anyhow::Result<CallToolResult> {
    let stages = super::out::stages(stages)?;
    let job = VehicleServiceClient::new(channel)
        .down(VehicleDownRequest {
            manifest: path_ref(resolved_workspace),
            stages,
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_args_default_to_every_stage() {
        let a: VehicleStageArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/ws/v" })).unwrap();
        assert!(a.stages.is_empty());
        assert!(up_confirmation("/ws/v", &a.stages).contains("(every stage)"));
    }

    #[test]
    fn confirmations_end_with_the_shared_host_line() {
        for t in [
            up_confirmation("/ws/v", &["compute".into()]),
            down_confirmation("/ws/v", &[]),
        ] {
            assert!(t.ends_with("This affects anyone using this host's testbed. Confirm?"));
        }
    }
}
