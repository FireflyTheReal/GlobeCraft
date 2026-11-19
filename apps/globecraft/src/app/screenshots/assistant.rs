//! The assistant against a local fake model.
use super::*;

#[test]
#[ignore = "manual: drives the real application against a local fake model; see the module documentation"]
fn assistant_runs_commands_end_to_end() {
    let port = fake_model(vec![
        json!({"choices": [{"finish_reason": "tool_calls", "message": {"role": "assistant",
        "content": "Flying to Paris and turning on the grid.",
        "tool_calls": [
            {"id": "a", "type": "function", "function": {"name": "run_command",
                "arguments": "{\"command\":\"camera.flyTo\",\"params\":{\"lat\":48.8566,\"lon\":2.3522,\"altitude_km\":15}}"}},
            {"id": "b", "type": "function", "function": {"name": "run_command",
                "arguments": "{\"command\":\"layer.show\",\"params\":{\"layer\":\"grid\",\"visible\":true}}"}}
        ]}}], "usage": {"prompt_tokens": 1500, "completion_tokens": 60}}),
        json!({"choices": [{"finish_reason": "stop", "message": {"role": "assistant",
            "content": "You're over **Paris** at 15 km, with the latitude/longitude grid on."}}],
            "usage": {"prompt_tokens": 1700, "completion_tokens": 25}}),
    ]);
    let mut h = harness(ThemePreference::Light);
    let app = h.state_mut();
    app.settings.workspace.open_right(PaneTab::Assistant);
    app.ensure_agent();
    let mut config = app.assistant_config("custom");
    config.base_url = format!("http://127.0.0.1:{port}/v1");
    config.model = "fake".into();
    app.apply_assistant_config(config);
    app.assistant_send("Show me Paris with a grid".into());
    for _ in 0..80 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !h.state().assistant.busy() {
            break;
        }
    }
    let app = h.state();
    assert!(!app.assistant.busy(), "the conversation finished");
    assert!(
        app.layer(globecraft_core::layers::MapLayer::Grid),
        "grid shown by the assistant"
    );
    let transcript = &app.assistant.agent.as_ref().expect("agent").transcript;
    assert!(
        transcript
            .iter()
            .any(|e| matches!(e, globecraft_assistant::Entry::Assistant(t) if t.contains("Paris"))),
        "{transcript:?}"
    );
    // The turn ends with its tokens (no price: the fake model's family is unknown).
    assert!(
        matches!(transcript.last(), Some(globecraft_assistant::Entry::Usage(u))
            if u.usage.all_input_tokens() == 3200 && u.cost_usd.is_none()),
        "{transcript:?}"
    );
    settle(&mut h, 5.0);
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    save(&mut h, &dir, "06-assistant-conversation");
    let cam = h.state().cam;
    assert!(
        (cam.lat - 48.8566).abs() < 0.5 && (cam.lon - 2.3522).abs() < 0.5,
        "{cam:?}"
    );
}
