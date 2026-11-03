//! `RunService` tools. A run is what `experiment up` or `vehicle up` brought up on this host.
//! - `run_list` → `List`: every run, with its id, workspace and start time.
//! - `run_get` → `Get`: one run; an omitted id means the only run.

use rmcp::model::CallToolResult;
use vxedge_api::v1::run_service_client::RunServiceClient;
use vxedge_api::v1::{RunGetRequest, RunListRequest};

/// Optional run addressing, shared by every tool that acts on a run.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    /// The run id from `run_list`. Omit it to mean the only run on this host. vxedged refuses
    /// a stale id with `ERROR_REASON_RUN_MISMATCH`, and a call with no run up with
    /// `ERROR_REASON_NO_ACTIVE_RUN`.
    #[serde(default)]
    pub run_id: Option<String>,
}

pub async fn list(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let resp = RunServiceClient::new(channel)
        .list(RunListRequest {})
        .await?
        .into_inner();
    super::out::json(&resp)
}

pub async fn get(
    channel: tonic::transport::Channel,
    args: RunArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = RunServiceClient::new(channel)
        .get(RunGetRequest {
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_args_are_optional() {
        let a: RunArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(a.run_id, None);
        let a: RunArgs = serde_json::from_value(serde_json::json!({ "run_id": "r1" })).unwrap();
        assert_eq!(a.run_id.as_deref(), Some("r1"));
    }
}
