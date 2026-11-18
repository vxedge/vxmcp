//! `VehicleService` tools, each one RPC. A vehicle workspace names its parts; vxedged plans
//! them onto zone and compute nodes.
//! - `vehicle_status` → `Status`: each stage and node, observed on the host.
//! - `vehicle_up` / `vehicle_down` → `Up` / `Down`, behind `crate::confirm`. Up runs the same
//!   preflight as `workspace_plan` first, in an instance slot. Down acts on the workspace's run,
//!   or the one `run_id` names, and tears stages down in reverse order, best effort.

use rmcp::model::CallToolResult;
use vxedge_api::v1::vehicle_service_client::VehicleServiceClient;
use vxedge_api::v1::{VehicleDownRequest, VehicleStatusRequest, VehicleUpRequest};

use super::experiment::{path_ref, resolve_workspace, stage_words};
use super::out::{run_words, slot_words};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct VehicleStatusArgs {
    /// Path to the vehicle workspace — the directory holding its documents and, for a package,
    /// its descriptor (`vxp.yaml`). Relative paths are resolved against this server's own
    /// working directory.
    pub workspace: String,
    /// The run, from `run_list`. Omit it to mean this workspace's run, or the only run.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct VehicleUpArgs {
    /// Path to the vehicle workspace — the directory holding its documents (and its `vxp.yaml`
    /// when it is a package; one that does not match its `vxp.lock` is refused with
    /// `ERROR_REASON_LOCK_MISMATCH`). You will be asked to confirm.
    pub workspace: String,
    /// Stages, as `vxedge vehicle up --stage` spells them: `gateway`, `zones`, `services`,
    /// `compute`, `signal-plane`. Omitted or empty means every stage.
    #[serde(default)]
    pub stages: Vec<String>,
    /// The instance slot to run in: one character, `0`-`9` or `a`-`z`. Omit it to keep this
    /// workspace's run's slot, or take the lowest free one. A slot another run holds is refused
    /// with `ERROR_REASON_HELD_BY_RUN`.
    #[serde(default)]
    pub instance: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct VehicleDownArgs {
    /// Path to the vehicle workspace. You will be asked to confirm.
    pub workspace: String,
    /// Stages, spelled as for `vehicle_up`. Omitted or empty means every stage, in reverse
    /// order.
    #[serde(default)]
    pub stages: Vec<String>,
    /// The run to tear down, from `run_list`. Omit it to mean this workspace's run; vxedged
    /// refuses with `ERROR_REASON_NO_ACTIVE_RUN` when it has none and
    /// `ERROR_REASON_RUN_AMBIGUOUS` when it has several.
    #[serde(default)]
    pub run_id: Option<String>,
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: VehicleStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = VehicleServiceClient::new(channel)
        .status(VehicleStatusRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

/// The question the human sees before `vehicle_up` runs, for an already-resolved path.
pub fn up_confirmation(resolved_workspace: &str, stages: &[String], instance: &str) -> String {
    format!(
        "Bring up the vehicle at `{resolved_workspace}` ({}, {}): provisions its gateway side and \
         the zone and compute nodes its parts are planned onto. vxedged checks the host first and \
         refuses if it can't run it, another job holds it or the slot is taken. This affects \
         anyone using this host's testbed. Confirm?",
        stage_words(stages),
        slot_words(instance)
    )
}

/// The question the human sees before `vehicle_down` runs, for an already-resolved path.
pub fn down_confirmation(
    resolved_workspace: &str,
    stages: &[String],
    run_id: Option<&str>,
) -> String {
    format!(
        "Tear down {} of the vehicle at `{resolved_workspace}` ({}): stops its traffic and \
         measurements, then its services, signal plane, compute and zone nodes and its gateway \
         side, in reverse order. vxedged refuses if that run isn't up. This affects anyone using \
         this host's testbed. Confirm?",
        run_words(run_id),
        stage_words(stages)
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn up(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    stages: &[String],
    instance: String,
) -> anyhow::Result<CallToolResult> {
    let stages = super::out::stages(stages)?;
    let job = VehicleServiceClient::new(channel)
        .up(VehicleUpRequest {
            manifest: path_ref(resolved_workspace),
            stages,
            instance,
            ..Default::default()
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
    run_id: Option<String>,
) -> anyhow::Result<CallToolResult> {
    let stages = super::out::stages(stages)?;
    let job = VehicleServiceClient::new(channel)
        .down(VehicleDownRequest {
            manifest: path_ref(resolved_workspace),
            stages,
            run: super::out::run_ref(run_id),
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
        let a: VehicleUpArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/ws/v" })).unwrap();
        assert!(a.stages.is_empty() && a.instance.is_none());
        assert!(
            up_confirmation("/ws/v", &a.stages, "").contains("(every stage, in this workspace's")
        );
        let d: VehicleDownArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/ws/v", "run_id": "r2" }))
                .unwrap();
        assert!(
            down_confirmation("/ws/v", &d.stages, d.run_id.as_deref())
                .starts_with("Tear down run `r2` of the vehicle at `/ws/v` (every stage)")
        );
    }

    #[test]
    fn confirmations_end_with_the_shared_host_line() {
        for t in [
            up_confirmation("/ws/v", &["compute".into()], "1"),
            down_confirmation("/ws/v", &[], None),
        ] {
            assert!(t.ends_with("This affects anyone using this host's testbed. Confirm?"));
        }
    }
}
