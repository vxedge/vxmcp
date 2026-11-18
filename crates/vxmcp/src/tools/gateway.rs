//! `gateway_status` — a direct 1:1 call to `GatewayService.Status`, the same RPC
//! `vxedge status` calls: a run's gateway side, with the slices, link impairments, ingress
//! filters and tunnels observed on the kernel.

use rmcp::model::CallToolResult;
use vxedge_api::v1::GatewayStatusRequest;
use vxedge_api::v1::gateway_service_client::GatewayServiceClient;

use super::run::RunArgs;

pub async fn status(
    channel: tonic::transport::Channel,
    args: RunArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = GatewayServiceClient::new(channel)
        .status(GatewayStatusRequest {
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}
