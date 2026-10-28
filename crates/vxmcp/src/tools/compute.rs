//! `compute_list` — a direct 1:1 call to `ComputeService.List`, the same RPC
//! `vxedge compute list` calls. Takes no arguments: the daemon lists the compute nodes the
//! active experiment declares.

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::compute_service_client::ComputeServiceClient;
use vxedge_api::v1::{ComputeListRequest, ResourceStatus};

pub async fn list(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let mut client = ComputeServiceClient::new(channel);
    let resp = client.list(ComputeListRequest {}).await?.into_inner();
    Ok(CallToolResult::success(vec![ContentBlock::text(render(
        &resp.nodes,
    ))]))
}

fn render(nodes: &[ResourceStatus]) -> String {
    // Same empty-is-an-answer rule as `zone.rs`.
    if nodes.is_empty() {
        return "no compute nodes reported".to_string();
    }
    nodes
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
    fn renders_one_line_per_node() {
        let nodes = vec![ResourceStatus {
            vxid: "hpc-1".into(),
            state: 3,
            detail: "running".into(),
            ..Default::default()
        }];
        assert_eq!(render(&nodes), "hpc-1 [state=3] running");
    }

    #[test]
    fn says_so_when_no_nodes_are_declared() {
        assert_eq!(render(&[]), "no compute nodes reported");
    }
}
