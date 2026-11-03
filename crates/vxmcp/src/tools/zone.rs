//! `zone_list` — a direct 1:1 call to `ZoneService.List`, the same RPC `vxedge zone list`
//! calls: the zones a run declares, each with its observed state and typed node status.

use rmcp::model::CallToolResult;
use vxedge_api::v1::ZoneListRequest;
use vxedge_api::v1::zone_service_client::ZoneServiceClient;

use super::run::RunArgs;

pub async fn list(
    channel: tonic::transport::Channel,
    args: RunArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = ZoneServiceClient::new(channel)
        .list(ZoneListRequest {
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}
