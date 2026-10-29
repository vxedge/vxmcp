//! Human confirmation for mutating tools (`TASKS.md` #3, design in
//! `docs/superpowers/specs/2026-10-29-mrtr-confirmation-design.md`).
//!
//! Every mutating tool calls [`Confirmations::gate`] before its one vxAPI RPC. Round 1 (no
//! `requestState`) asks the human through an MCP 2026-07-28 multi-round-trip form elicitation
//! and hands back a ticket; round 2 (the client's retry) runs only if that ticket is live,
//! belongs to this exact call, and the answer is `accept`. Decisions the owner made, kept here:
//! only a human confirms (no argument skips it), fail closed when the client can't elicit, no
//! unattended mode.
//!
//! Tickets are single-use random ids in an in-process map. `requestState` is caller-controlled
//! input, so it is only ever used as a map key and never parsed for meaning — no signing is
//! needed while each client gets its own long-lived stdio `vxmcp` process. A remote transport
//! would move this to `rmcp`'s `RequestStateCodec` plus a used-ticket set.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rmcp::ErrorData as McpError;
use rmcp::model::{
    CallToolResult, ClientCapabilities, ContentBlock, ElicitRequest, ElicitRequestParams,
    ElicitResult, ElicitationAction, ElicitationCapability, ElicitationSchema,
    FormElicitationCapability, InputRequest, InputRequiredResult, InputResponses,
};

/// Key of the one elicitation in `inputRequests` / `inputResponses`.
pub const CONFIRM_KEY: &str = "confirm";
/// How long a human has to answer before the ticket is void.
pub const TICKET_TTL: Duration = Duration::from_secs(300);

const INVALID: &str = "confirmation is invalid or expired; call the tool again to be asked";

/// What a mutating tool does next.
#[derive(Debug)]
pub enum Gate {
    /// Round 1: return this to the client, which asks its user and retries.
    Ask(InputRequiredResult),
    /// Round 2, accepted: make the RPC.
    Proceed,
    /// Round 2, declined or cancelled: return this; nothing runs.
    NotConfirmed(CallToolResult),
}

struct Ticket {
    tool: &'static str,
    args: serde_json::Value,
    expires_at: Instant,
}

#[derive(Default)]
pub struct Confirmations {
    tickets: Mutex<HashMap<String, Ticket>>,
}

impl Confirmations {
    pub fn new() -> Self {
        Self::default()
    }

    /// `tool` and `args` identify the call being confirmed: `args` must be the *resolved*
    /// arguments the RPC will use, so a ticket can't be spent on a different call.
    pub fn gate(
        &self,
        caps: Option<&ClientCapabilities>,
        tool: &'static str,
        args: &serde_json::Value,
        message: &str,
        request_state: Option<String>,
        input_responses: Option<InputResponses>,
    ) -> Result<Gate, McpError> {
        self.gate_at(
            Instant::now(),
            caps,
            tool,
            args,
            message,
            request_state,
            input_responses,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn gate_at(
        &self,
        now: Instant,
        caps: Option<&ClientCapabilities>,
        tool: &'static str,
        args: &serde_json::Value,
        message: &str,
        request_state: Option<String>,
        input_responses: Option<InputResponses>,
    ) -> Result<Gate, McpError> {
        let Some(ticket_id) = request_state else {
            return self.ask(now, caps, tool, args, message);
        };
        // Single use: gone on the first retry that presents it, whatever happens next.
        let ticket = self
            .tickets
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&ticket_id);
        let answer = input_responses
            .and_then(|mut r| r.remove(CONFIRM_KEY))
            .and_then(|v| serde_json::from_value::<ElicitResult>(v).ok())
            .ok_or_else(invalid)?;
        // Decline, cancel, and any action a future spec adds: only `accept` runs the call. A "no"
        // stands whatever the ticket's state — answering it with "call again to be asked" would
        // send the agent back to a human who just said no.
        if !matches!(answer.action, ElicitationAction::Accept) {
            return Ok(Gate::NotConfirmed(CallToolResult::success(vec![
                ContentBlock::text("not confirmed; nothing was run"),
            ])));
        }
        let valid = ticket.is_some_and(|t| t.expires_at > now && t.tool == tool && &t.args == args);
        if !valid {
            return Err(invalid());
        }
        Ok(Gate::Proceed)
    }

    fn ask(
        &self,
        now: Instant,
        caps: Option<&ClientCapabilities>,
        tool: &'static str,
        args: &serde_json::Value,
        message: &str,
    ) -> Result<Gate, McpError> {
        if !can_elicit_form(caps) {
            let mut required = ClientCapabilities::default();
            let mut elicitation = ElicitationCapability::new();
            elicitation.form = Some(FormElicitationCapability::new());
            required.elicitation = Some(elicitation);
            return Err(McpError::missing_required_client_capability(required));
        }
        let ticket_id = new_ticket_id()?;
        {
            let mut tickets = self.tickets.lock().unwrap_or_else(|p| p.into_inner());
            tickets.retain(|_, t| t.expires_at > now);
            tickets.insert(
                ticket_id.clone(),
                Ticket {
                    tool,
                    args: args.clone(),
                    expires_at: now + TICKET_TTL,
                },
            );
        }
        let request = ElicitRequest::new(ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: message.to_string(),
            requested_schema: ElicitationSchema::new(BTreeMap::new()),
        });
        let requests =
            BTreeMap::from([(CONFIRM_KEY.to_string(), InputRequest::Elicitation(request))]);
        Ok(Gate::Ask(InputRequiredResult::new(
            Some(requests),
            Some(ticket_id),
        )))
    }
}

/// Form elicitation is what the confirmation needs. Per spec, an `elicitation` capability that
/// names neither mode (`{}`, from clients predating the modes) means form.
fn can_elicit_form(caps: Option<&ClientCapabilities>) -> bool {
    caps.and_then(|c| c.elicitation.as_ref())
        .is_some_and(|e| e.form.is_some() || (e.form.is_none() && e.url.is_none()))
}

fn new_ticket_id() -> Result<String, McpError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| McpError::internal_error(format!("no randomness for a ticket: {e}"), None))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn invalid() -> McpError {
    McpError::invalid_params(INVALID, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{
        ClientCapabilities, ElicitRequestParams, ElicitationCapability, ErrorCode,
        FormElicitationCapability, InputRequest, InputResponses, UrlElicitationCapability,
    };
    use serde_json::json;
    use std::time::{Duration, Instant};

    const TEXT: &str = "Stop the `ota` service on this host. Confirm?";

    fn form_caps() -> ClientCapabilities {
        let mut caps = ClientCapabilities::default();
        let mut e = ElicitationCapability::new();
        e.form = Some(FormElicitationCapability::new());
        caps.elicitation = Some(e);
        caps
    }

    fn answer(action: &str) -> InputResponses {
        InputResponses::from([(CONFIRM_KEY.to_string(), json!({ "action": action }))])
    }

    fn accept() -> InputResponses {
        answer("accept")
    }

    fn ota() -> serde_json::Value {
        json!({ "service_type": "ota" })
    }

    /// Round 1 for `svc_down`/`ota` at `now`; returns the ticket.
    fn ask_at(c: &Confirmations, now: Instant) -> String {
        match c
            .gate_at(
                now,
                Some(&form_caps()),
                "svc_down",
                &ota(),
                TEXT,
                None,
                None,
            )
            .unwrap()
        {
            Gate::Ask(r) => r.request_state.unwrap(),
            other => panic!("expected Ask, got {other:?}"),
        }
    }

    fn ask(c: &Confirmations) -> String {
        ask_at(c, Instant::now())
    }

    fn retry(
        c: &Confirmations,
        tool: &'static str,
        args: &serde_json::Value,
        ticket: &str,
        responses: Option<InputResponses>,
    ) -> Result<Gate, rmcp::ErrorData> {
        c.gate(
            Some(&form_caps()),
            tool,
            args,
            TEXT,
            Some(ticket.to_string()),
            responses,
        )
    }

    fn assert_invalid(r: Result<Gate, rmcp::ErrorData>) {
        let e = r.expect_err("expected the invalid-confirmation error");
        assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            e.message,
            "confirmation is invalid or expired; call the tool again to be asked"
        );
    }

    #[test]
    fn round_one_asks_with_one_form_elicitation_and_a_ticket() {
        let c = Confirmations::new();
        let r = match c
            .gate(Some(&form_caps()), "svc_down", &ota(), TEXT, None, None)
            .unwrap()
        {
            Gate::Ask(r) => r,
            other => panic!("expected Ask, got {other:?}"),
        };
        let reqs = r.input_requests.expect("input requests");
        assert_eq!(reqs.keys().collect::<Vec<_>>(), vec![CONFIRM_KEY]);
        match &reqs[CONFIRM_KEY] {
            InputRequest::Elicitation(req) => match &req.params {
                ElicitRequestParams::FormElicitationParams { message, .. } => {
                    assert_eq!(message, TEXT)
                }
                other => panic!("expected a form elicitation, got {other:?}"),
            },
            other => panic!("expected an elicitation, got {other:?}"),
        }
        assert!(r.request_state.expect("ticket").len() >= 32);
    }

    #[test]
    fn accept_with_the_same_call_proceeds() {
        let c = Confirmations::new();
        let t = ask(&c);
        assert!(matches!(
            retry(&c, "svc_down", &ota(), &t, Some(accept())),
            Ok(Gate::Proceed)
        ));
    }

    #[test]
    fn decline_and_cancel_do_not_run() {
        let c = Confirmations::new();
        for action in ["decline", "cancel"] {
            let t = ask(&c);
            match retry(&c, "svc_down", &ota(), &t, Some(answer(action))).unwrap() {
                Gate::NotConfirmed(r) => {
                    let text = serde_json::to_string(&r.content).unwrap();
                    assert!(text.contains("not confirmed; nothing was run"), "{text}");
                }
                other => panic!("{action}: expected NotConfirmed, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_ticket_works_once() {
        let c = Confirmations::new();
        let t = ask(&c);
        assert!(matches!(
            retry(&c, "svc_down", &ota(), &t, Some(accept())),
            Ok(Gate::Proceed)
        ));
        assert_invalid(retry(&c, "svc_down", &ota(), &t, Some(accept())));
    }

    #[test]
    fn expired_ticket_is_refused() {
        let c = Confirmations::new();
        let t0 = Instant::now();
        let t = ask_at(&c, t0);
        assert_invalid(c.gate_at(
            t0 + TICKET_TTL + Duration::from_secs(1),
            Some(&form_caps()),
            "svc_down",
            &ota(),
            TEXT,
            Some(t),
            Some(accept()),
        ));
    }

    #[test]
    fn a_decline_stands_even_on_an_expired_or_unknown_ticket() {
        // "No" must never come back as "call again to be asked": nothing runs on a decline, so
        // the ticket's state doesn't matter for it.
        let c = Confirmations::new();
        let t0 = Instant::now();
        let t = ask_at(&c, t0);
        let expired = c.gate_at(
            t0 + TICKET_TTL + Duration::from_secs(1),
            Some(&form_caps()),
            "svc_down",
            &ota(),
            TEXT,
            Some(t),
            Some(answer("decline")),
        );
        assert!(matches!(expired, Ok(Gate::NotConfirmed(_))), "{expired:?}");
        let unknown = retry(
            &c,
            "svc_down",
            &ota(),
            "not-a-ticket",
            Some(answer("cancel")),
        );
        assert!(matches!(unknown, Ok(Gate::NotConfirmed(_))), "{unknown:?}");
    }

    #[test]
    fn ticket_for_another_tool_or_other_args_is_refused() {
        let c = Confirmations::new();
        let t = ask(&c);
        assert_invalid(retry(&c, "svc_up", &ota(), &t, Some(accept())));
        let t = ask(&c);
        assert_invalid(retry(
            &c,
            "svc_down",
            &json!({ "service_type": "telemetry" }),
            &t,
            Some(accept()),
        ));
    }

    #[test]
    fn unknown_ticket_is_refused() {
        let c = Confirmations::new();
        assert_invalid(retry(
            &c,
            "svc_down",
            &ota(),
            "not-a-ticket",
            Some(accept()),
        ));
    }

    #[test]
    fn missing_confirm_response_is_refused() {
        let c = Confirmations::new();
        let t = ask(&c);
        assert_invalid(retry(&c, "svc_down", &ota(), &t, None));
        let t = ask(&c);
        let other =
            InputResponses::from([("something-else".to_string(), json!({ "action": "accept" }))]);
        assert_invalid(retry(&c, "svc_down", &ota(), &t, Some(other)));
    }

    #[test]
    fn malformed_confirm_response_is_refused() {
        let c = Confirmations::new();
        for bad in [json!("yes"), json!({ "action": 7 })] {
            let t = ask(&c);
            let responses = InputResponses::from([(CONFIRM_KEY.to_string(), bad)]);
            assert_invalid(retry(&c, "svc_down", &ota(), &t, Some(responses)));
        }
    }

    #[test]
    fn empty_elicitation_capability_counts_as_form() {
        let c = Confirmations::new();
        let mut caps = ClientCapabilities::default();
        caps.elicitation = Some(ElicitationCapability::new());
        assert!(matches!(
            c.gate(Some(&caps), "svc_down", &ota(), TEXT, None, None),
            Ok(Gate::Ask(_))
        ));
    }

    #[test]
    fn url_only_or_missing_capabilities_are_refused() {
        let c = Confirmations::new();
        let mut url_only = ClientCapabilities::default();
        let mut e = ElicitationCapability::new();
        e.url = Some(UrlElicitationCapability::new());
        url_only.elicitation = Some(e);
        let none = ClientCapabilities::default();
        for caps in [None, Some(&none), Some(&url_only)] {
            let err = c
                .gate(caps, "svc_down", &ota(), TEXT, None, None)
                .expect_err("expected the capability error");
            assert_eq!(err.code, ErrorCode::MISSING_REQUIRED_CLIENT_CAPABILITY);
        }
    }
}
