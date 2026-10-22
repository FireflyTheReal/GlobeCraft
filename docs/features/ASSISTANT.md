# AI assistant

Open it from Home › AI Assistant, View › Panes, Ctrl/⌘+Shift+A, or by typing a question in
Command Search. Ask about the view or describe what to do: the assistant answers and acts by
running GlobeCraft commands (flying, searching, layers, basemaps, placemarks, the ruler,
worlds, units). Each call is listed in the conversation with its result.

## Providers

| Provider | Wire format | Key |
| --- | --- | --- |
| Anthropic | Messages | `ANTHROPIC_API_KEY` or typed |
| OpenAI | Chat Completions | `OPENAI_API_KEY` or typed |
| Google Gemini | `streamGenerateContent` | `GEMINI_API_KEY` / `GOOGLE_API_KEY` or typed |
| OpenRouter | Chat Completions | `OPENROUTER_API_KEY` or typed |
| Ollama (local) | Chat Completions | none (`OLLAMA_BASE_URL` sets the server) |
| OpenAI-compatible | Chat Completions | optional (`OPENAI_COMPATIBLE_BASE_URL`, `OPENAI_COMPATIBLE_API_KEY`) |

Choose the provider, model, address and key with the gear in the pane (or Settings ›
AI Provider). **List** asks the provider for its models. Replies stream in as they are
written. Requests go only to `https://` addresses, or `http://` on this computer or the local
network.

## Privacy and safety

- Your message, a short description of the view (camera, world, basemap, visible layers,
  counts of places and ruler points, search results) and command results are sent to the
  provider you choose. Nothing is sent until you write.
- Keys typed in the pane stay in memory unless you tick **Remember API keys on this
  computer**; they are then stored in `assistant-keys.json` next to `settings.json`, readable
  only by your user account on Linux, macOS and BSD. Keys are never written to `settings.json`.
- Commands that read or write files (open, save, screenshot) wait for your approval; you can
  allow them for the rest of the conversation. Commands that discard work (Clear All, Quit)
  are asked about every time. Commands that only open interface panes are not available to
  the assistant.
- Place names, search results and placemark descriptions are passed to the model as data,
  and the system prompt tells it not to follow instructions found in them.

## Design

`crates/globecraft-assistant` is a sans-I/O agent adapted from RustGIS's assistant
(Apache-2.0): it builds requests and reads replies as plain data, runs tool calls through a
`Host` trait, retries rate limits and overloads with back-off, compacts long conversations by
summarising them, and recovers from a panic in a tool without closing the app. The desktop
sends requests on its service runtime (`globecraft_data::model_http`) and applies results on
egui's thread, where `App::run` executes commands.

Tests in `crates/globecraft-assistant/src/tests.rs` cover every wire format with canned
replies; `assistant_runs_commands_end_to_end` in `apps/globecraft` drives the real application
against a local fake model.
