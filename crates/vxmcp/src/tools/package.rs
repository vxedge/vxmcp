//! `PackageService` tools, each one RPC.
//! - `package_inspect` → `Inspect`: a package's typed descriptor (parameters with bounds,
//!   metrics, criteria, campaign, measurements, entities), from a folder, a packed file, or the
//!   digest of one in this host's cache.
//! - `package_list` → `List`: the packages in this host's cache.
//! - `package_pull` → `Pull`, behind `crate::confirm`: fetch a package by digest into the cache,
//!   under the host's trust policy.
//! - `package_import` → `Import`, behind `crate::confirm`: upload a packed package from the host
//!   `vxmcp` runs on into the daemon's cache, for when the two share no filesystem.

use rmcp::model::CallToolResult;
use sha2::Digest;
use vxedge_api::v1::manifest_ref::Source;
use vxedge_api::v1::package_service_client::PackageServiceClient;
use vxedge_api::v1::{
    ManifestRef, PackageImportChunk, PackageInspectRequest, PackageListRequest, PackagePullRequest,
};

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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PackageImportArgs {
    /// A packed `.vxp` file on the host `vxmcp` runs on. Use it when `vxedged` runs elsewhere and
    /// cannot read this host's files; on the same host, `package_inspect` and the up tools take the
    /// path directly. You will be asked to confirm.
    pub file: String,
}

/// The bytes go to `vxedged` this many at a time, well under gRPC's default 4 MiB message limit.
const IMPORT_CHUNK: usize = 1 << 20;

/// A packed file's contents and their digest, `sha256:<hex>`, read before the human is asked, so
/// the question names exactly what will be sent. `vxedged` refuses bytes that hash to anything
/// else, so a file changed after the question is never imported under its digest.
pub fn read_packed(file: &str) -> anyhow::Result<(String, Vec<u8>)> {
    let path = resolve_workspace(file);
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("cannot read the packed package `{path}`: {e}"))?;
    let digest = format!(
        "sha256:{}",
        super::result::hex(&sha2::Sha256::digest(&bytes))
    );
    Ok((digest, bytes))
}

/// The question the human sees before `package_import` runs.
pub fn import_confirmation(file: &str, digest: &str, size: usize) -> String {
    format!(
        "Upload the packed package `{file}` ({digest}, {size} bytes) from the host vxmcp runs on \
         into the package cache of the host vxedged runs on. vxedged checks the bytes against the \
         digest and that they are a canonical packed package. This writes to that host. Confirm?"
    )
}

pub async fn import(
    channel: tonic::transport::Channel,
    digest: String,
    bytes: Vec<u8>,
) -> anyhow::Result<CallToolResult> {
    let job = PackageServiceClient::new(channel)
        .import(tokio_stream::iter(import_chunks(digest, &bytes)))
        .await?
        .into_inner();
    super::out::json(&job)
}

/// The chunks of one upload: the digest in the first only. An empty file is still one chunk, so
/// the digest always reaches `vxedged`.
fn import_chunks(digest: String, bytes: &[u8]) -> Vec<PackageImportChunk> {
    let mut chunks: Vec<PackageImportChunk> = bytes
        .chunks(IMPORT_CHUNK)
        .map(|data| PackageImportChunk {
            digest: String::new(),
            data: data.to_vec(),
        })
        .collect();
    if chunks.is_empty() {
        chunks.push(PackageImportChunk::default());
    }
    chunks[0].digest = digest;
    chunks
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
    fn an_import_names_its_digest_in_the_first_chunk_only() {
        let bytes = vec![7u8; IMPORT_CHUNK * 2 + 3];
        let chunks = import_chunks("sha256:d".into(), &bytes);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].digest, "sha256:d");
        assert!(chunks[1..].iter().all(|c| c.digest.is_empty()));
        assert_eq!(
            chunks.iter().map(|c| c.data.len()).sum::<usize>(),
            bytes.len()
        );
        let empty = import_chunks("sha256:e".into(), &[]);
        assert_eq!((empty.len(), empty[0].digest.as_str()), (1, "sha256:e"));
    }

    #[test]
    fn a_packed_file_is_read_with_its_digest() {
        let dir = std::env::temp_dir().join(format!("vxmcp-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p.vxp");
        std::fs::write(&f, b"abc").unwrap();
        let (digest, bytes) = read_packed(f.to_str().unwrap()).unwrap();
        // sha256("abc")
        assert_eq!(
            digest,
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(bytes, b"abc");
        assert!(read_packed(dir.join("missing.vxp").to_str().unwrap()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
