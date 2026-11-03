//! `gateway_status` — a direct 1:1 call to `GatewayService.Status`, the same RPC
//! `vxedge status` calls. Takes no arguments: the daemon reports the one gateway it manages,
//! with the slices, link impairments, ingress filters and tunnels observed on the kernel.

use rmcp::model::CallToolResult;
use vxedge_api::v1::GatewayStatusRequest;
use vxedge_api::v1::gateway_service_client::GatewayServiceClient;

pub async fn status(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let resp = GatewayServiceClient::new(channel)
        .status(GatewayStatusRequest {})
        .await?
        .into_inner();
    super::out::json(&resp)
}
