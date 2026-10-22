//! Conversation tests with a scripted host and canned provider replies; no network.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::provider::{HttpResponse, Turn};
use crate::{Agent, Config, Entry, Host, Step};
use serde_json::{Value, json};

#[derive(Default)]
struct FakeHost {
    ran: Vec<(String, Value)>,
    context: String,
}

impl Host for FakeHost {
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        self.ran.push((id.to_string(), params.clone()));
        match id {
            "info.view" => Ok(json!({"camera": {"lat": 1.0, "lon": 2.0}})),
            "camera.flyTo" if params.get("lat").is_none() => Err("lat is required".into()),
            _ => Ok(json!({"ok": true})),
        }
    }
    fn context(&self) -> String {
        self.context.clone()
    }
}

fn config(provider: &str) -> Config {
    let mut c = Config::from_env(provider);
    c.key = "test-key".into();
    c.model = "test-model".into();
    c
}

fn ok(body: Value) -> Result<HttpResponse, crate::Error> {
    Ok(HttpResponse {
        status: 200,
        body: body.to_string().into_bytes(),
        headers: vec![],
    })
}

fn anthropic_tool_use(name: &str, input: Value) -> Value {
    json!({
        "content": [
            {"type": "text", "text": "On it."},
            {"type": "tool_use", "id": "toolu_1", "name": name, "input": input}
        ],
        "stop_reason": "tool_use"
    })
}

fn anthropic_text(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "stop_reason": "end_turn"})
}

#[test]
fn a_tool_call_runs_and_the_answer_follows() {
    let mut host = FakeHost {
        context: "camera 1,2".into(),
        ..Default::default()
    };
    let mut agent = Agent::new(config("anthropic"));
    let Step::Send(req) = agent.ask("Fly to Paris", &host) else {
        panic!("expected a request")
    };
    assert!(req.url.ends_with("/v1/messages"));
    let body: Value = serde_json::from_str(req.body.as_deref().unwrap()).unwrap();
    assert!(
        body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("<view>\ncamera 1,2\n</view>")
    );
    assert!(
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "run_command")
    );
    let g = agent.generation();
    let step = agent.receive(
        g,
        ok(anthropic_tool_use("run_command", json!({"command": "camera.flyTo", "params": {"lat": 48.85, "lon": 2.35, "altitude_km": 15}}))),
        &mut host,
    );
    assert!(
        matches!(step, Step::Send(_)),
        "results go back to the model"
    );
    assert_eq!(
        host.ran,
        [(
            "camera.flyTo".to_string(),
            json!({"lat": 48.85, "lon": 2.35, "altitude_km": 15})
        )]
    );
    let step = agent.receive(
        agent.generation(),
        ok(anthropic_text("You're over Paris.")),
        &mut host,
    );
    assert_eq!(step, Step::Done);
    assert!(!agent.busy());
    assert!(
        matches!(agent.transcript.last(), Some(Entry::Assistant(t)) if t == "You're over Paris.")
    );
    // The unchanged context isn't sent twice.
    let Step::Send(req) = agent.ask("and now?", &host) else {
        panic!()
    };
    assert!(!req.body.unwrap().contains("<view>\ncamera 1,2"));
}

#[test]
fn file_commands_wait_for_approval_and_can_be_declined() {
    let mut host = FakeHost::default();
    let mut agent = Agent::new(config("anthropic"));
    let _ = agent.ask("Take a screenshot", &host);
    let step = agent.receive(
        agent.generation(),
        ok(anthropic_tool_use(
            "run_command",
            json!({"command": "file.screenshot"}),
        )),
        &mut host,
    );
    assert_eq!(step, Step::Confirm);
    let (what, why, always) = agent.pending().unwrap();
    assert!(what.starts_with("file.screenshot"));
    assert!(why.contains("files"));
    assert!(always);
    let step = agent.decide(false, false, &mut host);
    assert!(matches!(step, Step::Send(_)));
    assert!(host.ran.is_empty(), "declined calls never run");
    assert!(matches!(
        agent.transcript.last(),
        Some(Entry::Tool { error: true, .. })
    ));
}

#[test]
fn interface_commands_and_unknown_ids_fail_without_running() {
    let mut host = FakeHost::default();
    let mut agent = Agent::new(config("openai"));
    let Step::Send(req) = agent.ask("open settings", &host) else {
        panic!()
    };
    assert!(req.url.ends_with("/chat/completions"));
    let reply = json!({"choices": [{"finish_reason": "tool_calls", "message": {"role": "assistant", "content": null,
    "tool_calls": [
        {"id": "a", "type": "function", "function": {"name": "run_command", "arguments": "{\"command\":\"ui.settings\"}"}},
        {"id": "b", "type": "function", "function": {"name": "run_command", "arguments": "{\"command\":\"nope.nope\"}"}}
    ]}}]});
    let step = agent.receive(agent.generation(), ok(reply), &mut host);
    assert!(matches!(step, Step::Send(_)));
    assert!(host.ran.is_empty());
    let Some(Turn::Results(results)) = agent.turns().last() else {
        panic!()
    };
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.error));
}

#[test]
fn streamed_gemini_replies_are_assembled_and_previewed() {
    let mut host = FakeHost::default();
    let mut agent = Agent::new(config("gemini"));
    let Step::Send(req) = agent.ask("Where am I?", &host) else {
        panic!()
    };
    assert!(req.url.contains(":streamGenerateContent?alt=sse"));
    let chunks = [
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Over the \"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Atlantic.\"}]},\"finishReason\":\"STOP\"}]}\n\n",
    ];
    let g = agent.generation();
    assert!(agent.stream(g, chunks[0].as_bytes()));
    assert_eq!(agent.preview(), Some("Over the "));
    assert!(agent.stream(g, chunks[1].as_bytes()));
    let body = chunks.concat().into_bytes();
    let step = agent.receive(
        g,
        Ok(HttpResponse {
            status: 200,
            body,
            headers: vec![],
        }),
        &mut host,
    );
    assert_eq!(step, Step::Done);
    assert!(
        matches!(agent.transcript.last(), Some(Entry::Assistant(t)) if t == "Over the Atlantic.")
    );
}

#[test]
fn overload_is_retried_then_reported() {
    let mut host = FakeHost::default();
    let mut agent = Agent::new(config("anthropic"));
    let _ = agent.ask("hi", &host);
    let busy = || {
        Ok(HttpResponse {
            status: 529,
            body: b"{\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}"
                .to_vec(),
            headers: vec![("retry-after".into(), "0".into())],
        })
    };
    for _ in 0..crate::agent::MAX_RETRIES {
        let step = agent.receive(agent.generation(), busy(), &mut host);
        assert!(matches!(step, Step::Retry(_)));
        assert!(matches!(agent.retry(agent.generation()), Step::Send(_)));
    }
    assert_eq!(
        agent.receive(agent.generation(), busy(), &mut host),
        Step::Done
    );
    assert!(matches!(agent.transcript.last(), Some(Entry::Error(e)) if e.status == Some(529)));
    assert!(!agent.busy());
    // The failed message was dropped, so a new one starts a clean conversation.
    assert!(agent.turns().is_empty());
}

#[test]
fn stop_discards_late_replies() {
    let mut host = FakeHost::default();
    let mut agent = Agent::new(config("anthropic"));
    let _ = agent.ask("hi", &host);
    let g = agent.generation();
    agent.stop();
    assert!(!agent.busy());
    assert_eq!(
        agent.receive(g, ok(anthropic_text("late")), &mut host),
        Step::Done
    );
    assert!(
        !agent
            .transcript
            .iter()
            .any(|e| matches!(e, Entry::Assistant(_)))
    );
}

#[test]
fn a_missing_key_is_reported_before_any_request() {
    let host = FakeHost::default();
    let mut c = config("anthropic");
    c.key.clear();
    let mut agent = Agent::new(c);
    assert_eq!(agent.ask("hi", &host), Step::Done);
    assert!(
        matches!(agent.transcript.last(), Some(Entry::Error(e)) if e.detail == "Enter an API key.")
    );
    assert!(!agent.busy());
}

#[test]
fn plain_http_is_refused_except_for_local_servers() {
    let mut c = config("custom");
    c.base_url = "http://example.com/v1".into();
    assert!(c.missing().is_some());
    c.base_url = "http://192.168.1.4:1234/v1".into();
    assert!(c.missing().is_none());
}
