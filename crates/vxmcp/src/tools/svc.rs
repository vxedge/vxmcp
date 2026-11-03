//! `SvcService` tools, each one RPC. `service_type` is `ota` or `telemetry`; vxedged rejects any
//! other value with `ERROR_REASON_UNKNOWN_SERVICE_TYPE`, so the list is not duplicated here.
//! - `svc_status` → `Status`. With a workspace, vxedged reports the service the workspace
//!   declares, including where it is placed; without one, whether it runs on this host.
//! - `svc_up` → `Up`, behind `crate::confirm`: starts the service the workspace declares, in
//!   the active run.
//! - `svc_down` → `Down`, behind `crate::confirm`: stops that service on the host, whichever
//!   experiment started it, so that is exactly what its confirmation says.

use rmcp::model::CallToolResult;
use vxedge_api::v1::svc_service_client::SvcServiceClient;
use vxedge_api::v1::{SvcDownRequest, SvcStatusRequest, SvcUpRequest};

use super::experiment::{path_ref, resolve_workspace};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SvcStatusArgs {
    /// Which experiment service to report on: `ota` or `telemetry`.
    pub service_type: String,
    /// The experiment workspace that declares the service. Omit it to ask only whether the
    /// service runs on this host.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: SvcStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = SvcServiceClient::new(channel)
        .status(SvcStatusRequest {
            service_type: args.service_type,
            experiment: args.workspace.and_then(|w| path_ref(resolve_workspace(&w))),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SvcUpArgs {
    /// Which experiment service to start: `ota` or `telemetry`. You will be asked to confirm.
    pub service_type: String,
    /// The experiment workspace that declares the service.
    pub workspace: String,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
}

/// The question the human sees before `svc_up` runs, for an already-resolved path.
pub fn up_confirmation(service_type: &str, resolved_workspace: &str) -> String {
    format!(
        "Start the `{service_type}` service that `{resolved_workspace}` declares, in the run \
         active on this host. vxedged refuses if no run is up or another job holds the host. \
         This affects anyone using this host's testbed. Confirm?"
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn up(
    channel: tonic::transport::Channel,
    service_type: String,
    resolved_workspace: String,
    run_id: Option<String>,
) -> anyhow::Result<CallToolResult> {
    let job = SvcServiceClient::new(channel)
        .up(SvcUpRequest {
            service_type,
            experiment: path_ref(resolved_workspace),
            run: super::out::run_ref(run_id),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SvcDownArgs {
    /// Which experiment service to stop on this host: `ota` or `telemetry`. You will be asked
    /// to confirm.
    pub service_type: String,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
}

/// The question the human sees before `svc_down` runs. It must say what `SvcService.Down`
/// really does (design doc: confirmation text matches the daemon, not the RPC's name).
pub fn down_confirmation(service_type: &str) -> String {
    format!(
        "Stop the `{service_type}` service on this host, whichever experiment started it. This \
         affects anyone using this host's testbed. Confirm?"
    )
}

pub async fn down(
    channel: tonic::transport::Channel,
    args: SvcDownArgs,
) -> anyhow::Result<CallToolResult> {
    let job = SvcServiceClient::new(channel)
        .down(SvcDownRequest {
            service_type: args.service_type,
            // vxedged's Down finds the placed service from the active run, not from this.
            experiment: None,
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
    fn status_args_need_only_a_service_type() {
        let args: SvcStatusArgs =
            serde_json::from_value(serde_json::json!({ "service_type": "ota" })).unwrap();
        assert_eq!(args.service_type, "ota");
        assert!(args.workspace.is_none() && args.run_id.is_none());
    }

    #[test]
    fn down_args_deserialize_from_a_bare_service_type_field() {
        let args: SvcDownArgs =
            serde_json::from_value(serde_json::json!({ "service_type": "ota" })).unwrap();
        assert_eq!(args.service_type, "ota");
    }

    #[test]
    fn down_confirmation_names_the_service_and_the_shared_host() {
        assert_eq!(
            down_confirmation("ota"),
            "Stop the `ota` service on this host, whichever experiment started it. This affects \
             anyone using this host's testbed. Confirm?"
        );
    }

    #[test]
    fn up_confirmation_names_the_service_and_the_workspace() {
        let t = up_confirmation("telemetry", "/ws/e");
        assert!(t.starts_with("Start the `telemetry` service that `/ws/e` declares"));
        assert!(t.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }
}
