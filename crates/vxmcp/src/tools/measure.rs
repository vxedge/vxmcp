//! `MeasurementService` tools, each one RPC, against a run that is up.
//! - `measure_status` → `Status`: each declared measurement with its entity and kind, as running,
//!   finished or failed.
//! - `measure_start` / `measure_stop` → `Start` / `Stop`, behind `crate::confirm`. Start takes a
//!   `request_id`: a retry with the same id returns the original job instead of starting the
//!   senders again.

use rmcp::model::CallToolResult;
use vxedge_api::v1::measurement_service_client::MeasurementServiceClient;
use vxedge_api::v1::{MeasurementStartRequest, MeasurementStatusRequest, MeasurementStopRequest};

use super::experiment::{path_ref, resolve_workspace};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeasureStatusArgs {
    /// Path to the experiment workspace that declares the measurements.
    pub workspace: String,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeasureArgs {
    /// Path to the experiment workspace that declares the measurements. You will be asked to
    /// confirm.
    pub workspace: String,
    /// Measurement ids to act on. Omitted or empty means every measurement the workspace
    /// declares.
    #[serde(default)]
    pub ids: Vec<String>,
    /// `measure_start` only: how long each sender runs, in seconds. Omitted means the
    /// experiment's `method.duration_s`.
    #[serde(default)]
    pub duration_s: Option<u32>,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// `measure_start` only: an id of your choosing. Retrying with the same id returns the
    /// original job instead of starting the senders again.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn id_words(ids: &[String]) -> String {
    if ids.is_empty() {
        "every measurement".to_string()
    } else {
        format!("measurements {}", ids.join(", "))
    }
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: MeasureStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = MeasurementServiceClient::new(channel)
        .status(MeasurementStatusRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

/// The question the human sees before `measure_start` runs, for an already-resolved path.
pub fn start_confirmation(resolved_workspace: &str, ids: &[String]) -> String {
    format!(
        "Start {} that `{resolved_workspace}` declares, in the run active on this host: probes \
         run inside the nodes and send through the slices until each sender's duration ends or \
         measure_stop stops them. This affects anyone using this host's testbed. Confirm?",
        id_words(ids)
    )
}

/// The question the human sees before `measure_stop` runs, for an already-resolved path.
pub fn stop_confirmation(resolved_workspace: &str, ids: &[String]) -> String {
    format!(
        "Stop {} that `{resolved_workspace}` declares, in the run active on this host. This \
         affects anyone using this host's testbed. Confirm?",
        id_words(ids)
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn start(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: MeasureArgs,
) -> anyhow::Result<CallToolResult> {
    let job = MeasurementServiceClient::new(channel)
        .start(MeasurementStartRequest {
            manifest: path_ref(resolved_workspace),
            ids: args.ids,
            run: super::out::run_ref(args.run_id),
            duration_s: args.duration_s.unwrap_or(0),
            request_id: args.request_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn stop(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: MeasureArgs,
) -> anyhow::Result<CallToolResult> {
    let job = MeasurementServiceClient::new(channel)
        .stop(MeasurementStopRequest {
            manifest: path_ref(resolved_workspace),
            ids: args.ids,
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_need_only_a_workspace() {
        let a: MeasureArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/ws/e" })).unwrap();
        assert!(a.ids.is_empty() && a.duration_s.is_none() && a.request_id.is_none());
    }

    #[test]
    fn confirmations_name_the_measurements() {
        assert!(start_confirmation("/ws/e", &[]).starts_with("Start every measurement"));
        let t = stop_confirmation("/ws/e", &["ctl-rtt".into()]);
        assert!(t.starts_with("Stop measurements ctl-rtt that `/ws/e` declares"));
        assert!(t.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }
}
