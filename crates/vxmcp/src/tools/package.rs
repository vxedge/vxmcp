//! `PackageService` tools, each one RPC.
//! - `package_inspect` → `Inspect`: a package's typed descriptor (parameters with bounds,
//!   metrics, criteria, campaign, measurements, entities), from a folder, a packed file, or the
//!   digest of one in this host's cache.
//! - `package_list` → `List`: the packages in this host's cache.
//! - `package_pull` → `Pull`, behind `crate::confirm`: fetch a package by digest into the cache,
//!   under the host's trust policy.

use rmcp::model::CallToolResult;
use vxedge_api::v1::manifest_ref::Source;
use vxedge_api::v1::package_service_client::PackageServiceClient;
use vxedge_api::v1::{ManifestRef, PackageInspectRequest, PackageListRequest, PackagePullRequest};

use super::experiment::resolve_workspace;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PackageInspectArgs {
    /// A package folder or packed `.vxp` file on this host, or `sha256:<hex>`: the digest of a
    /// package in this host's cache (`package_list`).
    pub package: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PackagePullArgs {
    /// `<repository>@sha256:<hex>`: the package's registry repository and its package digest.
    /// You will be asked to confirm.
    pub reference: String,
    /// An id of your choosing. Retrying with the same id returns the original job.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// A digest addresses the cache; anything else is a path on this host.
pub fn package_ref(package: &str) -> Option<ManifestRef> {
    let source = if package.starts_with("sha256:") {
        Source::Package(package.to_string())
    } else {
        Source::Path(resolve_workspace(package))
    };
    Some(ManifestRef {
        source: Some(source),
        ..Default::default()
    })
}

pub async fn inspect(
    channel: tonic::transport::Channel,
    args: PackageInspectArgs,
) -> anyhow::Result<CallToolResult> {
    let info = PackageServiceClient::new(channel)
        .inspect(PackageInspectRequest {
            package: package_ref(&args.package),
        })
        .await?
        .into_inner();
    super::out::json(&info)
}

pub async fn list(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let list = PackageServiceClient::new(channel)
        .list(PackageListRequest {})
        .await?
        .into_inner();
    super::out::json(&list)
}

/// The question the human sees before `package_pull` runs.
pub fn pull_confirmation(reference: &str) -> String {
    format!(
        "Pull the package `{reference}` from its registry into this host's package cache. The \
         bytes are checked against the digest, and the host's trust policy applies before \
         anything is fetched. This writes to this host. Confirm?"
    )
}

pub async fn pull(
    channel: tonic::transport::Channel,
    args: PackagePullArgs,
) -> anyhow::Result<CallToolResult> {
    let job = PackageServiceClient::new(channel)
        .pull(PackagePullRequest {
            reference: args.reference,
            request_id: args.request_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_addresses_the_cache_and_anything_else_is_a_path() {
        let digest = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            package_ref(&digest).unwrap().source,
            Some(Source::Package(digest.clone()))
        );
        assert!(matches!(
            package_ref("/no/such/pkg").unwrap().source,
            Some(Source::Path(p)) if p == "/no/such/pkg"
        ));
    }
}
