//! `compute_list` — a direct 1:1 call to `ComputeService.List`, the same RPC
//! `vxedge compute list` calls: the compute nodes a run declares, each with its observed state
//! and typed node status.

use rmcp::model::CallToolResult;
use vxedge_api::v1::ComputeListRequest;
use vxedge_api::v1::compute_service_client::ComputeServiceClient;

use super::run::RunArgs;

pub async fn list(
    channel: tonic::transport::Channel,
    args: RunArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = ComputeServiceClient::new(channel)
        .list(ComputeListRequest {
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}
