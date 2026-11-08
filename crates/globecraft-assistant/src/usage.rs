//! Token usage as the providers report it, and an estimate of what a turn cost.
//!
//! Every reply says how many tokens it read and wrote (Anthropic's `usage`, Chat Completions'
//! `usage` when asked for with `stream_options`, Gemini's `usageMetadata`). The agent adds them
//! up over the requests one user message sets off and shows the total with the turn, priced
//! from [`PRICES`] when the model's family is known. It is an estimate: prices change, and
//! discounts, batch rates and long-context surcharges aren't modelled.

use crate::provider::{self, Config};
use serde_json::Value;

/// Tokens read and written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Input read at the full rate (with Anthropic, cache writes too).
    pub input_tokens: u64,
    /// Input served from the provider's prompt cache, billed at a lower rate.
    pub cached_input_tokens: u64,
    /// Output, with any reasoning tokens.
    pub output_tokens: u64,
}

impl Usage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0 && self.cached_input_tokens == 0 && self.output_tokens == 0
    }

    /// All input, cached or not.
    pub fn all_input_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.cached_input_tokens)
    }

    fn add(&mut self, other: Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(other.cached_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }

    /// The larger of each count: streamed counts are running totals, so the last is the largest.
    fn max(self, other: Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens.max(other.input_tokens),
            cached_input_tokens: self.cached_input_tokens.max(other.cached_input_tokens),
            output_tokens: self.output_tokens.max(other.output_tokens),
        }
    }
}

/// What one user message cost: the tokens of every request it set off, and their estimated
/// price in US dollars (`None` when a model's price isn't known).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TurnUsage {
    pub usage: Usage,
    pub cost_usd: Option<f64>,
}

impl TurnUsage {
    /// A fresh turn: nothing used, priced at zero until a request of an unknown model.
    pub(crate) fn new() -> Self {
        TurnUsage {
            usage: Usage::default(),
            cost_usd: Some(0.0),
        }
    }

    /// Adds a request's usage, priced for the model it went to.
    pub(crate) fn add(&mut self, usage: Usage, config: &Config) {
        self.usage.add(usage);
        self.cost_usd = match (self.cost_usd, estimate_usd(config, usage)) {
            (Some(sum), Some(cost)) => Some(sum + cost),
            _ => None,
        };
    }
}

/// Prices in US dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub cached_input: f64,
    pub output: f64,
}

const fn price(input: f64, cached_input: f64, output: f64) -> Price {
    Price {
        input,
        cached_input,
        output,
    }
}

/// List prices per model family, APPROXIMATE and dated 2026-11: what the providers' pricing
/// pages said for the standard tier, rounded. They only feed the "about $…" estimate shown with
/// each turn; update them here (and only here) when they drift. A model matches the first entry
/// whose every part its id contains (case-insensitive), so specific entries come first. Models
/// no entry matches show tokens only.
pub const PRICES: &[(&[&str], Price)] = &[
    // Anthropic.
    (&["claude", "opus"], price(5.0, 0.50, 25.0)),
    (&["claude", "sonnet"], price(3.0, 0.30, 15.0)),
    (&["claude", "haiku"], price(1.0, 0.10, 5.0)),
    // OpenAI.
    (&["gpt-5", "nano"], price(0.05, 0.005, 0.40)),
    (&["gpt-5", "mini"], price(0.25, 0.025, 2.0)),
    (&["gpt-5"], price(1.25, 0.125, 10.0)),
    // Google.
    (&["gemini", "flash-lite"], price(0.10, 0.01, 0.40)),
    (&["gemini", "flash"], price(0.30, 0.03, 2.50)),
    (&["gemini", "pro"], price(2.0, 0.20, 12.0)),
];

/// The approximate price of `model`, if its family is in [`PRICES`].
pub fn price_of(model: &str) -> Option<Price> {
    let model = model.to_ascii_lowercase();
    PRICES
        .iter()
        .find(|(parts, _)| parts.iter().all(|p| model.contains(p)))
        .map(|(_, price)| *price)
}

/// The estimated cost of `usage` with `config`'s model, in US dollars. A model running on the
/// user's own machine (Ollama) costs nothing to price, whatever its name.
pub fn estimate_usd(config: &Config, usage: Usage) -> Option<f64> {
    if config.provider == "ollama" {
        return None;
    }
    let p = price_of(&config.model)?;
    let per_token = |tokens: u64, usd_per_million: f64| tokens as f64 * usd_per_million / 1e6;
    Some(
        per_token(usage.input_tokens, p.input)
            + per_token(usage.cached_input_tokens, p.cached_input)
            + per_token(usage.output_tokens, p.output),
    )
}

/// The usage a reply body reports (an event stream or plain JSON), if it reports any.
pub fn read(body: &[u8]) -> Option<Usage> {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim_start();
    let events = if trimmed.starts_with('{') {
        serde_json::from_str::<Value>(trimmed)
            .map(|v| vec![v])
            .unwrap_or_default()
    } else {
        provider::sse_events(&text)
    };
    let usage = events
        .iter()
        .filter_map(from_event)
        .fold(Usage::default(), Usage::max);
    (!usage.is_empty()).then_some(usage)
}

/// The counts in one response or stream event, in any of the three wire formats.
fn from_event(e: &Value) -> Option<Usage> {
    let n = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
    // Gemini: the prompt count includes cached content; thoughts are billed as output.
    if let Some(u) = e.get("usageMetadata").filter(|u| u.is_object()) {
        let cached = n(u, "cachedContentTokenCount");
        return Some(Usage {
            input_tokens: n(u, "promptTokenCount").saturating_sub(cached),
            cached_input_tokens: cached,
            output_tokens: n(u, "candidatesTokenCount").saturating_add(n(u, "thoughtsTokenCount")),
        });
    }
    // Anthropic puts the input in `message_start` and the output in `message_delta`.
    let u = e
        .get("usage")
        .filter(|u| u.is_object())
        .or_else(|| e.pointer("/message/usage").filter(|u| u.is_object()))?;
    if u.get("prompt_tokens").is_some() || u.get("completion_tokens").is_some() {
        // Chat Completions: the prompt count includes cached tokens.
        let cached = u
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        return Some(Usage {
            input_tokens: n(u, "prompt_tokens").saturating_sub(cached),
            cached_input_tokens: cached,
            output_tokens: n(u, "completion_tokens"),
        });
    }
    Some(Usage {
        input_tokens: n(u, "input_tokens").saturating_add(n(u, "cache_creation_input_tokens")),
        cached_input_tokens: n(u, "cache_read_input_tokens"),
        output_tokens: n(u, "output_tokens"),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn config(provider: &str, model: &str) -> Config {
        let mut c = Config::from_env(provider);
        c.model = model.into();
        c
    }

    #[test]
    fn anthropic_streams_add_input_from_the_start_and_output_from_the_end() {
        let body = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1200,\"cache_read_input_tokens\":3000,\"cache_creation_input_tokens\":100,\"output_tokens\":1}}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":250}}\n\n",
        );
        assert_eq!(
            read(body.as_bytes()),
            Some(Usage {
                input_tokens: 1300,
                cached_input_tokens: 3000,
                output_tokens: 250
            })
        );
    }

    #[test]
    fn chat_completions_and_gemini_usage_is_read() {
        let openai = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":900,\"completion_tokens\":40,\"prompt_tokens_details\":{\"cached_tokens\":500}}}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(
            read(openai.as_bytes()),
            Some(Usage {
                input_tokens: 400,
                cached_input_tokens: 500,
                output_tokens: 40
            })
        );
        let gemini = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"a\"}]}}],\"usageMetadata\":{\"promptTokenCount\":700,\"candidatesTokenCount\":2}}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":700,\"candidatesTokenCount\":30,\"thoughtsTokenCount\":10}}\n\n",
        );
        assert_eq!(
            read(gemini.as_bytes()),
            Some(Usage {
                input_tokens: 700,
                cached_input_tokens: 0,
                output_tokens: 40
            })
        );
        // A plain JSON reply, and one that says nothing about usage.
        let plain = br#"{"content":[],"usage":{"input_tokens":10,"output_tokens":5}}"#;
        assert_eq!(read(plain).map(|u| u.output_tokens), Some(5));
        assert_eq!(read(br#"{"content":[]}"#), None);
        assert_eq!(read(b"not json"), None);
    }

    #[test]
    fn known_families_are_priced_and_others_show_tokens_only() {
        let usage = Usage {
            input_tokens: 1_000_000,
            cached_input_tokens: 1_000_000,
            output_tokens: 100_000,
        };
        let opus = estimate_usd(&config("anthropic", "claude-opus-5-5"), usage).unwrap();
        assert!((opus - (5.0 + 0.5 + 2.5)).abs() < 1e-9, "{opus}");
        // Specific entries win over the family's general one.
        assert_eq!(price_of("gpt-5-mini"), Some(price(0.25, 0.025, 2.0)));
        assert_eq!(price_of("GPT-5.5"), Some(price(1.25, 0.125, 10.0)));
        assert_eq!(
            price_of("models/gemini-3.8-flash-lite"),
            Some(price(0.10, 0.01, 0.40))
        );
        assert_eq!(
            price_of("anthropic/claude-sonnet-5"),
            Some(price(3.0, 0.30, 15.0))
        );
        assert_eq!(
            estimate_usd(&config("openrouter", "openrouter/auto"), usage),
            None
        );
        assert_eq!(estimate_usd(&config("custom", "qwen3-32b"), usage), None);
        // A local model is never priced, even under a known name.
        assert_eq!(estimate_usd(&config("ollama", "gpt-5"), usage), None);
    }

    #[test]
    fn a_turn_adds_requests_and_loses_its_price_with_an_unknown_model() {
        let usage = Usage {
            input_tokens: 2_000,
            cached_input_tokens: 0,
            output_tokens: 100,
        };
        let mut turn = TurnUsage::new();
        turn.add(usage, &config("anthropic", "claude-haiku-5"));
        turn.add(usage, &config("anthropic", "claude-haiku-5"));
        assert_eq!(turn.usage.input_tokens, 4_000);
        assert_eq!(turn.usage.output_tokens, 200);
        let cost = turn.cost_usd.unwrap();
        assert!((cost - 0.005).abs() < 1e-12, "{cost}");
        turn.add(usage, &config("custom", "my-model"));
        assert_eq!(turn.cost_usd, None);
        assert_eq!(turn.usage.all_input_tokens(), 6_000);
    }
}
