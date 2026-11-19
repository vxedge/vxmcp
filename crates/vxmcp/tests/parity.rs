//! Parity with vxAPI: every RPC in the pinned `vxedge-api` is reached by a tool or resource, or is
//! excluded in `rpc-exclusions.txt` with its reason.
//!
//! The RPCs come from the `.proto` files of the exact `vxedge-api` revision in `Cargo.lock`, found
//! through `cargo metadata`. What vxmcp reaches comes from its own source: every call of the form
//! `<Name>ServiceClient::new(..).<method>(`, which is how each tool and resource calls vxedged.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// `Service.Rpc` for every RPC the pinned protos declare.
fn proto_rpcs() -> BTreeSet<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let manifest = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "vxedge-api")
        .and_then(|p| p["manifest_path"].as_str())
        .expect("vxedge-api is a dependency");
    // crates/vxedge-api/Cargo.toml -> the repository root, whose proto/ the crate builds from.
    let root = Path::new(manifest).ancestors().nth(3).unwrap();
    let dir = root.join("proto/vxedge/v1");
    let mut rpcs = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "proto") {
            parse_proto(&std::fs::read_to_string(&path).unwrap(), &mut rpcs);
        }
    }
    assert!(!rpcs.is_empty(), "no RPCs found under {}", dir.display());
    rpcs
}

fn parse_proto(text: &str, rpcs: &mut BTreeSet<String>) {
    let mut service = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("service ") {
            service = rest.split_whitespace().next().map(str::to_owned);
        } else if let (Some(s), Some(rest)) = (&service, line.strip_prefix("rpc ")) {
            let name = rest.split('(').next().unwrap().trim();
            rpcs.insert(format!("{s}.{name}"));
        }
    }
}

/// `Service.Rpc` for every client call in vxmcp's source.
fn reached() -> BTreeSet<String> {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut calls = BTreeSet::new();
    for f in files {
        let s = std::fs::read_to_string(&f).unwrap();
        let mut from = 0;
        while let Some(at) = s[from..].find("ServiceClient::new(") {
            let start = from + at;
            let name_start = s[..start]
                .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .map_or(0, |i| i + 1);
            let service = &s[name_start..start];
            // Past the matching `)` of `new(`, then `.method(`.
            let mut i = start + "ServiceClient::new(".len();
            let mut depth = 1;
            while depth > 0 {
                match s.as_bytes()[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            let rest = s[i..].trim_start();
            if let Some(rest) = rest.strip_prefix('.') {
                let method: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                calls.insert(format!("{service}Service.{}", pascal(&method)));
            }
            from = i;
        }
    }
    calls
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn pascal(snake: &str) -> String {
    snake
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| {
                f.to_ascii_uppercase().to_string() + c.as_str()
            })
        })
        .collect()
}

/// `Service.Rpc` -> reason, from `rpc-exclusions.txt`.
fn exclusions() -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("rpc-exclusions.txt");
    let mut out = BTreeMap::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (rpc, reason) = line.split_once(':').expect("`Service.Rpc: reason`");
        assert!(
            !reason.trim().is_empty(),
            "{rpc} is excluded with no reason"
        );
        assert!(
            out.insert(rpc.trim().to_owned(), reason.trim().to_owned())
                .is_none(),
            "{rpc} twice"
        );
    }
    out
}

#[test]
fn every_rpc_has_a_tool_or_a_recorded_exclusion() {
    let (rpcs, reached, excluded) = (proto_rpcs(), reached(), exclusions());
    let unknown: Vec<_> = reached.difference(&rpcs).collect();
    assert!(
        unknown.is_empty(),
        "vxmcp calls RPCs the pinned protos do not declare: {unknown:?}"
    );
    let missing: Vec<_> = rpcs
        .iter()
        .filter(|r| !reached.contains(*r) && !excluded.contains_key(*r))
        .collect();
    assert!(
        missing.is_empty(),
        "RPCs with neither a tool nor an exclusion in rpc-exclusions.txt: {missing:?}"
    );
    let stale: Vec<_> = excluded
        .keys()
        .filter(|r| reached.contains(*r) || !rpcs.contains(*r))
        .collect();
    assert!(
        stale.is_empty(),
        "exclusions for RPCs that have a tool or no longer exist: {stale:?}"
    );
}

#[test]
fn a_proto_service_block_is_read_rpc_by_rpc() {
    let mut rpcs = BTreeSet::new();
    parse_proto(
        "service RunService {\n  // A comment.\n  rpc Get(RunGetRequest) returns (Run);\n  \
         rpc Down(RunDownRequest) returns (Job) {\n    option deprecated = true;\n  }\n}\n",
        &mut rpcs,
    );
    assert_eq!(
        rpcs.into_iter().collect::<Vec<_>>(),
        ["RunService.Down", "RunService.Get"]
    );
}
