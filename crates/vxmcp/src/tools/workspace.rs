//! Read tools an agent calls before it changes anything, each one RPC.
//! - `workspace_plan` → `WorkspaceService.Plan`: the checks `experiment up` and `vehicle up`
//!   make before touching the host, and the nodes they would provision, with nothing changed.
//! - `host_capabilities` → `HostService.Capabilities`: which backends and part kinds this host
//!   can run, and how many runs it holds at once.

use rmcp::model::CallToolResult;
use vxedge_api::v1::host_service_client::HostServiceClient;
use vxedge_api::v1::workspace_service_client::WorkspaceServiceClient;
use vxedge_api::v1::{HostCapabilitiesRequest, WorkspacePlanRequest};

use super::experiment::{path_ref, resolve_workspace};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkspaceArgs {
    /// Path to the experiment or vehicle workspace — the directory containing its `.vxp` file.
    /// Relative paths are resolved against this server's own working directory.
    pub workspace: String,
}

pub async fn plan(
    channel: tonic::transport::Channel,
    args: WorkspaceArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = WorkspaceServiceClient::new(channel)
        .plan(WorkspacePlanRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

pub async fn capabilities(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let resp = HostServiceClient::new(channel)
        .capabilities(HostCapabilitiesRequest {})
        .await?
        .into_inner();
    super::out::json(&resp)
}
