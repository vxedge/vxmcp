//! `NosCheckService` tools, each one RPC, all against a run that is up. The NOS check boots
//! the workspace's network OS image as a disposable QEMU guest.
//! - `noscheck_status` → `Status`: whether the instance runs.
//! - `noscheck_up` / `noscheck_down` → `Up` / `Down`, behind `crate::confirm`.
//! - `noscheck_verify` → `Verify`, behind `crate::confirm`: boots its own disposable instance,
//!   checks the switch config against the gateway plan field by field, and removes it. A
//!   `request_id` makes a retry return the original job.

use rmcp::model::CallToolResult;
use vxedge_api::v1::nos_check_service_client::NosCheckServiceClient;
use vxedge_api::v1::{
    NosCheckDownRequest, NosCheckStatusRequest, NosCheckUpRequest, NosCheckVerifyRequest,
};

use super::experiment::{path_ref, resolve_workspace};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NosCheckArgs {
    /// Path to the experiment workspace that declares the NOS check. Mutating tools ask you to
    /// confirm.
    pub workspace: String,
    /// The run, from `run_list`. Omit it to mean the only run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// `noscheck_verify` only: an id of your choosing. Retrying with the same id returns the
    /// original job instead of booting another instance.
    #[serde(default)]
    pub request_id: Option<String>,
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: NosCheckArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = NosCheckServiceClient::new(channel)
        .status(NosCheckStatusRequest {
            experiment: path_ref(resolve_workspace(&args.workspace)),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

/// What each mutating NOS check tool does, for its confirmation.
pub fn confirmation(verb: &str, resolved_workspace: &str) -> String {
    let what = match verb {
        "up" => "Boot the NOS check instance",
        "down" => "Stop the NOS check instance",
        _ => {
            "Boot a disposable NOS check instance, compare its switch config with the gateway \
             plan, then remove it, for"
        }
    };
    format!(
        "{what} that `{resolved_workspace}` declares, in the run active on this host. This \
         affects anyone using this host's testbed. Confirm?"
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn up(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: NosCheckArgs,
) -> anyhow::Result<CallToolResult> {
    let job = NosCheckServiceClient::new(channel)
        .up(NosCheckUpRequest {
            experiment: path_ref(resolved_workspace),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn down(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: NosCheckArgs,
) -> anyhow::Result<CallToolResult> {
    let job = NosCheckServiceClient::new(channel)
        .down(NosCheckDownRequest {
            experiment: path_ref(resolved_workspace),
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn verify(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: NosCheckArgs,
) -> anyhow::Result<CallToolResult> {
    let job = NosCheckServiceClient::new(channel)
        .verify(NosCheckVerifyRequest {
            experiment: path_ref(resolved_workspace),
            run: super::out::run_ref(args.run_id),
            request_id: args.request_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmations_say_what_each_verb_does() {
        assert!(confirmation("up", "/ws/e").starts_with("Boot the NOS check instance that"));
        assert!(confirmation("down", "/ws/e").starts_with("Stop the NOS check instance that"));
        let v = confirmation("verify", "/ws/e");
        assert!(v.contains("compare its switch config with the gateway plan"));
        assert!(v.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }
}
