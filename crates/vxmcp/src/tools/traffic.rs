//! `TrafficService` tools, each one RPC, all against a run that is up.
//! - `traffic_status` → `Status`: each profile as running, finished or failed, from the exit
//!   code its process left, so an agent knows when a load run is over.
//! - `traffic_up` / `traffic_down` → `Up` / `Down`, behind `crate::confirm`. Up takes a
//!   `request_id`: a retry with the same id returns the original job instead of starting the
//!   load twice.

use rmcp::model::CallToolResult;
use vxedge_api::v1::traffic_service_client::TrafficServiceClient;
use vxedge_api::v1::{TrafficDownRequest, TrafficStatusRequest, TrafficUpRequest};

use super::experiment::{path_ref, resolve_workspace};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TrafficStatusArgs {
    /// Path to the experiment workspace that declares the traffic profiles.
    pub workspace: String,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TrafficArgs {
    /// Path to the experiment workspace that declares the traffic profiles. You will be asked
    /// to confirm.
    pub workspace: String,
    /// Profile names to act on. Omitted or empty means every profile the workspace declares.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// `traffic_up` only: an id of your choosing. Retrying with the same id returns the
    /// original job instead of starting the load again.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn profile_words(profiles: &[String]) -> String {
    if profiles.is_empty() {
        "every traffic profile".to_string()
    } else {
        format!("traffic profiles {}", profiles.join(", "))
    }
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: TrafficStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = TrafficServiceClient::new(channel)
        .status(TrafficStatusRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

/// The question the human sees before `traffic_up` runs, for an already-resolved path.
pub fn up_confirmation(resolved_workspace: &str, profiles: &[String]) -> String {
    format!(
        "Start {} that `{resolved_workspace}` declares, in the run active on this host: load \
         generators send traffic through the slices until each profile's duration ends or \
         traffic_down stops them. This affects anyone using this host's testbed. Confirm?",
        profile_words(profiles)
    )
}

/// The question the human sees before `traffic_down` runs, for an already-resolved path.
pub fn down_confirmation(resolved_workspace: &str, profiles: &[String]) -> String {
    format!(
        "Stop {} that `{resolved_workspace}` declares, in the run active on this host. This \
         affects anyone using this host's testbed. Confirm?",
        profile_words(profiles)
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn up(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: TrafficArgs,
) -> anyhow::Result<CallToolResult> {
    let job = TrafficServiceClient::new(channel)
        .up(TrafficUpRequest {
            manifest: path_ref(resolved_workspace),
            profiles: args.profiles,
            run: super::out::run_ref(args.run_id),
            request_id: args.request_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn down(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: TrafficArgs,
) -> anyhow::Result<CallToolResult> {
    let job = TrafficServiceClient::new(channel)
        .down(TrafficDownRequest {
            manifest: path_ref(resolved_workspace),
            profiles: args.profiles,
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
        let a: TrafficArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/ws/e" })).unwrap();
        assert!(a.profiles.is_empty() && a.run_id.is_none() && a.request_id.is_none());
    }

    #[test]
    fn confirmations_name_the_profiles() {
        assert!(up_confirmation("/ws/e", &[]).starts_with("Start every traffic profile"));
        let t = down_confirmation("/ws/e", &["embb".into()]);
        assert!(t.starts_with("Stop traffic profiles embb that `/ws/e` declares"));
        assert!(t.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }
}
