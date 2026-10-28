//! `svc_status` — a direct 1:1 call to `SvcService.Status`. No CLI verb calls this RPC yet
//! (`vxedge ota`/`telemetry` only wrap `Up`/`Down`), but `vxedged` serves it. Only
//! `service_type` is sent: the daemon reads that field alone and validates it itself, so the
//! accepted values aren't duplicated here as a schema enum that could drift from it.

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::svc_service_client::SvcServiceClient;
use vxedge_api::v1::{ResourceStatus, SvcStatusRequest};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SvcStatusArgs {
    /// Which experiment service to report on — currently `ota` or `telemetry`. `vxedged`
    /// rejects anything else with an error naming the accepted values.
    pub service_type: String,
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: SvcStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let mut client = SvcServiceClient::new(channel);
    let resp = client
        .status(SvcStatusRequest {
            service_type: args.service_type,
            experiment: None,
        })
        .await?
        .into_inner();
    Ok(CallToolResult::success(vec![ContentBlock::text(render(
        &resp,
    ))]))
}

fn render(s: &ResourceStatus) -> String {
    format!("{} [state={}] {}", s.vxid, s.state, s.detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vxedge_api::v1::ResourceStatus;

    #[test]
    fn args_deserialize_from_a_bare_service_type_field() {
        let args: SvcStatusArgs =
            serde_json::from_value(serde_json::json!({ "service_type": "ota" })).unwrap();
        assert_eq!(args.service_type, "ota");
    }

    #[test]
    fn renders_the_single_status_line() {
        let s = ResourceStatus {
            vxid: "svc-ota".into(),
            state: 1,
            detail: "ota service not running".into(),
            ..Default::default()
        };
        assert_eq!(render(&s), "svc-ota [state=1] ota service not running");
    }
}
