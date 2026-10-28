//! `zone_list` — a direct 1:1 call to `ZoneService.List`, the same RPC `vxedge zone list`
//! calls. Takes no arguments: the daemon lists the zones the active experiment declares.

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::zone_service_client::ZoneServiceClient;
use vxedge_api::v1::{ResourceStatus, ZoneListRequest};

pub async fn list(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let mut client = ZoneServiceClient::new(channel);
    let resp = client.list(ZoneListRequest {}).await?.into_inner();
    Ok(CallToolResult::success(vec![ContentBlock::text(render(
        &resp.zones,
    ))]))
}

fn render(zones: &[ResourceStatus]) -> String {
    // An empty list is a real answer, not an error — say so rather than return empty text an
    // agent could mistake for a dropped response.
    if zones.is_empty() {
        return "no zones reported".to_string();
    }
    zones
        .iter()
        .map(|s| format!("{} [state={}] {}", s.vxid, s.state, s.detail))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vxedge_api::v1::ResourceStatus;

    #[test]
    fn renders_one_line_per_zone() {
        let zones = vec![
            ResourceStatus {
                vxid: "zone-front".into(),
                state: 3,
                detail: "running".into(),
                ..Default::default()
            },
            ResourceStatus {
                vxid: "zone-rear".into(),
                state: 1,
                detail: "stopped".into(),
                ..Default::default()
            },
        ];
        assert_eq!(
            render(&zones),
            "zone-front [state=3] running\nzone-rear [state=1] stopped"
        );
    }

    #[test]
    fn says_so_when_no_zones_are_declared() {
        assert_eq!(render(&[]), "no zones reported");
    }
}
