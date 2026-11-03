//! What every tool returns. A result is the RPC's own response in the canonical Protobuf JSON
//! mapping (`vxedge-api`'s `serde` feature): enum values by name, lowerCamelCase fields, so an
//! agent branches on `"state": "JOB_STATE_FAILED"`, never on an ordinal or on prose. The same
//! JSON goes in `structuredContent` and in a text block, for clients that read only text.
//!
//! A refusal from `vxedged` is a tool result with `isError: true`, not a JSON-RPC error, so the
//! model sees it: `{"error": {"code", "reason", "domain", "message", "metadata"}}`, where
//! `reason` is vxedged's `ErrorReason` name (from the status's `ErrorInfo`). A JSON-RPC error is
//! kept for this server's own faults and for a bad argument it can reject before any RPC.

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;
use tonic_types::StatusExt;
use vxedge_api::v1::{ExperimentStage, RunRef};

/// A bad tool argument, caught before any RPC is sent. Reaches the client as `invalid_params`.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ArgError(pub String);

/// The response as typed JSON.
pub fn json<T: serde::Serialize>(value: &T) -> anyhow::Result<CallToolResult> {
    Ok(CallToolResult::structured(serde_json::to_value(value)?))
}

/// Maps a tool body's outcome onto MCP: a vxedged refusal becomes a readable tool error, a bad
/// argument `invalid_params`, anything else `internal_error`.
pub fn finish(result: anyhow::Result<CallToolResult>) -> Result<CallToolResult, McpError> {
    match result {
        Ok(r) => Ok(r),
        Err(e) => {
            if let Some(status) = e.downcast_ref::<tonic::Status>() {
                Ok(rpc_error(status))
            } else if let Some(arg) = e.downcast_ref::<ArgError>() {
                Err(McpError::invalid_params(arg.0.clone(), None))
            } else {
                Err(McpError::internal_error(e.to_string(), None))
            }
        }
    }
}

/// A refusal as a tool error the agent can branch on.
pub fn rpc_error(status: &tonic::Status) -> CallToolResult {
    let details = status.get_error_details();
    let info = details.error_info();
    let metadata: serde_json::Map<String, serde_json::Value> = info
        .map(|i| {
            let mut m: Vec<_> = i.metadata.iter().collect();
            m.sort();
            m.into_iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect()
        })
        .unwrap_or_default();
    CallToolResult::structured_error(serde_json::json!({
        "error": {
            "code": code_name(status.code()),
            // Empty when the refusal carries no ErrorInfo: a daemon older than 0.5.0, or a
            // transport failure.
            "reason": info.map(|i| i.reason.clone()).unwrap_or_default(),
            "domain": info.map(|i| i.domain.clone()).unwrap_or_default(),
            "message": status.message(),
            "metadata": metadata,
        }
    }))
}

/// The canonical gRPC code name, e.g. `FAILED_PRECONDITION`.
fn code_name(code: tonic::Code) -> String {
    let camel = format!("{code:?}");
    let mut out = String::with_capacity(camel.len() + 4);
    for (i, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Addresses a run by id; empty or omitted means the only run, which vxedged resolves.
pub fn run_ref(run_id: Option<String>) -> Option<RunRef> {
    run_id
        .filter(|id| !id.is_empty())
        .map(|run_id| RunRef { run_id })
}

/// Stage names as the CLI spells them (`gateway`, `zones`, `services`, `compute`,
/// `signal-plane`) to `ExperimentStage` values. Empty means every stage.
pub fn stages(names: &[String]) -> anyhow::Result<Vec<i32>> {
    names
        .iter()
        .map(|n| {
            let enum_name = format!(
                "EXPERIMENT_STAGE_{}",
                n.to_ascii_uppercase().replace('-', "_")
            );
            match ExperimentStage::from_str_name(&enum_name) {
                Some(ExperimentStage::Unspecified) | None => Err(ArgError(format!(
                    "unknown stage `{n}`: use gateway, zones, services, compute or signal-plane"
                ))
                .into()),
                Some(s) => Ok(s as i32),
            }
        })
        .collect()
}

/// Checks stage names before the human is asked, so a typo never reaches a confirmation.
pub fn check_stages(names: &[String]) -> Result<(), McpError> {
    stages(names)
        .map(|_| ())
        .map_err(|e| McpError::invalid_params(e.to_string(), None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic_types::{ErrorDetails, StatusExt};

    #[test]
    fn a_refusal_carries_the_typed_reason() {
        let status = tonic::Status::with_error_details(
            tonic::Code::Aborted,
            "another lifecycle job is running",
            ErrorDetails::with_error_info(
                "ERROR_REASON_BUSY",
                "vxedge.v1",
                [("job_id".to_string(), "job-1-4".to_string())],
            ),
        );
        let r = rpc_error(&status);
        assert_eq!(r.is_error, Some(true));
        let v = r.structured_content.unwrap();
        assert_eq!(v["error"]["code"], "ABORTED");
        assert_eq!(v["error"]["reason"], "ERROR_REASON_BUSY");
        assert_eq!(v["error"]["domain"], "vxedge.v1");
        assert_eq!(v["error"]["metadata"]["job_id"], "job-1-4");
        assert_eq!(v["error"]["message"], "another lifecycle job is running");
    }

    #[test]
    fn a_refusal_without_error_info_has_an_empty_reason() {
        let v = rpc_error(&tonic::Status::not_found("unknown job `x`"))
            .structured_content
            .unwrap();
        assert_eq!(v["error"]["code"], "NOT_FOUND");
        assert_eq!(v["error"]["reason"], "");
    }

    #[test]
    fn finish_turns_a_status_into_a_tool_error_and_an_arg_error_into_invalid_params() {
        let r = finish(Err(tonic::Status::failed_precondition("no run").into())).unwrap();
        assert_eq!(r.is_error, Some(true));
        let e = finish(Err(ArgError("bad".into()).into())).unwrap_err();
        assert_eq!(e.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn code_names_are_canonical() {
        assert_eq!(
            code_name(tonic::Code::FailedPrecondition),
            "FAILED_PRECONDITION"
        );
        assert_eq!(code_name(tonic::Code::Ok), "OK");
    }

    #[test]
    fn stage_names_map_and_unknown_ones_are_refused() {
        assert_eq!(
            stages(&["gateway".into(), "signal-plane".into()]).unwrap(),
            vec![
                ExperimentStage::Gateway as i32,
                ExperimentStage::SignalPlane as i32
            ]
        );
        assert!(stages(&["unspecified".into()]).is_err());
        assert!(stages(&["bogus".into()]).is_err());
        assert!(stages(&[]).unwrap().is_empty());
    }

    #[test]
    fn an_empty_run_id_means_the_only_run() {
        assert_eq!(run_ref(Some(String::new())), None);
        assert_eq!(run_ref(None), None);
        assert_eq!(
            run_ref(Some("run-1".into())),
            Some(RunRef {
                run_id: "run-1".into()
            })
        );
    }

    #[test]
    fn json_results_name_enums() {
        let job = vxedge_api::v1::Job {
            id: "job-1-1".into(),
            state: vxedge_api::v1::JobState::Succeeded as i32,
            ..Default::default()
        };
        let r = json(&job).unwrap();
        assert_eq!(
            r.structured_content.unwrap()["state"],
            "JOB_STATE_SUCCEEDED"
        );
    }
}
