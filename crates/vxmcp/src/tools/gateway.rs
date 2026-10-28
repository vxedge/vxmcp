//! `gateway_status` — a direct 1:1 call to `GatewayService.Status`, the same RPC
//! `vxedge status` calls. Takes no arguments: the daemon reports the one gateway it manages.
//! Covers every section the CLI's own text output prints (slices, impairments, ingress filters,
//! tunnels), in this crate's `[state=N]` line convention (`experiment.rs`).

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::gateway_service_client::GatewayServiceClient;
use vxedge_api::v1::{GatewayStatus, GatewayStatusRequest};

pub async fn status(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let mut client = GatewayServiceClient::new(channel);
    let resp = client.status(GatewayStatusRequest {}).await?.into_inner();
    Ok(CallToolResult::success(vec![ContentBlock::text(render(
        &resp,
    ))]))
}

fn render(status: &GatewayStatus) -> String {
    let Some(top) = &status.status else {
        return "gateway: no status reported".to_string();
    };
    let mut lines = vec![format!("{} [state={}] {}", top.vxid, top.state, top.detail)];
    for s in &status.slices {
        lines.push(format!(
            "  slice {} [state={}] {}",
            s.slice_id, s.state, s.detail
        ));
    }
    for i in &status.impairments {
        lines.push(format!(
            "  impairment {} ({}) [state={}] {}",
            i.link, i.access_port, i.state, i.detail
        ));
    }
    for f in &status.ingress_filters {
        lines.push(format!(
            "  ingress filter {} ({}) [state={}] {}",
            f.node, f.access_port, f.state, f.detail
        ));
    }
    for t in &status.tunnels {
        // `kind` stays a bare TunnelKind ordinal, same as `state` — matches vxedge-cli's own
        // `({})` rendering of it.
        lines.push(format!(
            "  tunnel {} (kind={}) [state={}] handshake={} public_key={} {}",
            t.id, t.kind, t.state, t.handshake_established, t.public_key, t.detail
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vxedge_api::v1::{
        GatewayStatus, ImpairmentStatus, IngressFilterStatus, ResourceStatus, SliceStatus,
        TunnelStatus,
    };

    #[test]
    fn renders_top_line_and_every_observed_section() {
        let status = GatewayStatus {
            status: Some(ResourceStatus {
                vxid: "vxgateway".into(),
                state: 3,
                detail: "converged".into(),
                ..Default::default()
            }),
            slices: vec![SliceStatus {
                slice_id: "urllc".into(),
                state: 3,
                detail: "vlan 101".into(),
            }],
            impairments: vec![ImpairmentStatus {
                link: "zone-a".into(),
                access_port: "p1".into(),
                state: 4,
                detail: "egress only".into(),
            }],
            ingress_filters: vec![IngressFilterStatus {
                node: "ecu-1".into(),
                access_port: "p2".into(),
                state: 1,
                detail: "missing".into(),
            }],
            tunnels: vec![TunnelStatus {
                id: "wg0".into(),
                state: 3,
                public_key: "PUBKEY".into(),
                handshake_established: true,
                ..Default::default()
            }],
        };
        let out = render(&status);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "vxgateway [state=3] converged");
        assert_eq!(lines[1], "  slice urllc [state=3] vlan 101");
        assert_eq!(lines[2], "  impairment zone-a (p1) [state=4] egress only");
        assert_eq!(lines[3], "  ingress filter ecu-1 (p2) [state=1] missing");
        assert!(
            lines[4]
                .starts_with("  tunnel wg0 (kind=0) [state=3] handshake=true public_key=PUBKEY")
        );
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn says_so_when_the_daemon_reports_no_status() {
        assert_eq!(
            render(&GatewayStatus::default()),
            "gateway: no status reported"
        );
    }
}
