//! The AI assistant's session: the agent, its provider configuration, and the request in flight.
//! Requests go out on the service runtime ([`globecraft_data::model_http`]); this module turns
//! their results into agent steps on egui's thread, where commands run.
use super::App;
use super::commands::AssistantHost;
use globecraft_assistant::{
    Agent, Config, Error, HttpResponse, PRESETS, Step, TransportFailure, provider,
};
use globecraft_data::model_http::{self, ModelCall, ModelFailure, ModelRequest};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Most bytes of a model list read.
const MODEL_LIST_LIMIT: usize = 4 << 20;

#[derive(Default)]
pub(in crate::app) struct AssistantState {
    pub agent: Option<Agent>,
    pub input: String,
    /// Messages sent, for Up/Down in the input box.
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub show_settings: bool,
    /// "Allow such actions for this conversation" on the approval card.
    pub always: bool,
    /// The request in flight and the agent generation it belongs to.
    inflight: Option<(u64, ModelCall)>,
    /// A failed request waiting to be sent again.
    retry: Option<(u64, Instant)>,
    /// A question asked from elsewhere (the command palette), sent when the agent is free.
    queued: Option<String>,
    /// Keys typed this session, by provider (saved only when the user opts in).
    pub keys: BTreeMap<String, String>,
    keys_loaded: bool,
    /// Models the provider offers, once listed.
    pub models: Vec<String>,
    models_call: Option<(String, ModelCall)>,
    pub models_error: Option<String>,
}

impl AssistantState {
    /// Asks `question` as soon as the assistant is free.
    pub(in crate::app) fn queue(&mut self, question: String) {
        self.queued = Some(question);
    }

    pub(in crate::app) fn busy(&self) -> bool {
        self.agent.as_ref().is_some_and(Agent::busy)
    }

    pub(in crate::app) fn listing_models(&self) -> bool {
        self.models_call.is_some()
    }
}

fn to_request(req: globecraft_assistant::HttpRequest) -> ModelRequest {
    let limit = provider::reply_limit(&req);
    ModelRequest {
        method: req.method,
        url: req.url,
        headers: req.headers,
        body: req.body,
        limit,
        keep_headers: &provider::KEPT_HEADERS,
    }
}

fn to_response(
    result: Result<model_http::ModelResponse, ModelFailure>,
) -> Result<HttpResponse, Error> {
    match result {
        Ok(r) => Ok(HttpResponse {
            status: r.status,
            body: r.body,
            headers: r.headers,
        }),
        Err(ModelFailure::Connection(e)) => Err(Error::transport(TransportFailure::Connection, e)),
        Err(ModelFailure::Timeout(e)) => Err(Error::transport(TransportFailure::Timeout, e)),
        Err(ModelFailure::Interrupted(e)) => {
            Err(Error::transport(TransportFailure::Interrupted, e))
        }
    }
}

impl App {
    /// The configuration for `provider` from settings, the session's keys and the environment.
    pub(in crate::app) fn assistant_config(&mut self, provider_id: &str) -> Config {
        if !self.assistant.keys_loaded {
            self.assistant.keys_loaded = true;
            if self.settings.assistant.remember_keys {
                self.assistant.keys = globecraft_data::settings::assistant::load_keys();
            }
        }
        let mut config = Config::from_env(provider_id);
        let prefs = &self.settings.assistant;
        if let Some(model) = prefs.models.get(&config.provider) {
            config.model = model.clone();
        }
        if let Some(url) = prefs.base_urls.get(&config.provider) {
            config.base_url = url.clone();
        }
        if let Some(key) = self
            .assistant
            .keys
            .get(&config.provider)
            .filter(|k| !k.is_empty())
        {
            config.key = key.clone();
        }
        config
    }

    /// The provider to start with: the saved one, else the first with a key in the environment.
    fn initial_provider(&self) -> String {
        let saved = &self.settings.assistant.provider;
        if provider::preset(saved).is_some() {
            return saved.clone();
        }
        PRESETS
            .iter()
            .find(|p| {
                p.key_env
                    .iter()
                    .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
            })
            .map_or("anthropic", |p| p.id)
            .to_string()
    }

    /// Creates the agent on first use.
    pub(in crate::app) fn ensure_agent(&mut self) {
        if self.assistant.agent.is_none() {
            let provider = self.initial_provider();
            let config = self.assistant_config(&provider);
            self.assistant.agent = Some(Agent::new(config));
        }
    }

    /// Applies an edited configuration and remembers the choices (and the key, if asked to).
    pub(in crate::app) fn apply_assistant_config(&mut self, config: Config) {
        let prefs = &mut self.settings.assistant;
        prefs.provider = config.provider.clone();
        let preset = provider::preset(&config.provider);
        if preset.is_some_and(|p| p.model != config.model) {
            prefs
                .models
                .insert(config.provider.clone(), config.model.clone());
        } else {
            prefs.models.remove(&config.provider);
        }
        if preset.is_some_and(|p| p.base_url != config.base_url) {
            prefs
                .base_urls
                .insert(config.provider.clone(), config.base_url.clone());
        } else {
            prefs.base_urls.remove(&config.provider);
        }
        let key_changed = self
            .assistant
            .keys
            .get(&config.provider)
            .map(String::as_str)
            != Some(config.key.as_str());
        if key_changed {
            if config.key.is_empty() {
                self.assistant.keys.remove(&config.provider);
            } else {
                self.assistant
                    .keys
                    .insert(config.provider.clone(), config.key.clone());
            }
            self.save_assistant_keys();
        }
        if let Some(agent) = self.assistant.agent.as_mut() {
            if agent.config.provider != config.provider {
                self.assistant.models.clear();
                self.assistant.models_error = None;
            }
            agent.config = config;
        }
    }

    /// Writes the session's keys when the user chose to remember them, or removes the file.
    pub(in crate::app) fn save_assistant_keys(&mut self) {
        let keys = if self.settings.assistant.remember_keys {
            self.assistant.keys.clone()
        } else {
            BTreeMap::new()
        };
        self.runtime.handle().spawn_blocking(move || {
            if let Err(error) = globecraft_data::settings::assistant::save_keys(&keys) {
                eprintln!("Saving assistant keys failed: {error}");
            }
        });
    }

    /// Sends the user's message.
    pub(in crate::app) fn assistant_send(&mut self, text: String) {
        self.ensure_agent();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.assistant.history.last() != Some(&text) {
            self.assistant.history.push(text.clone());
        }
        self.assistant.history_pos = None;
        self.assistant.input.clear();
        let Some(mut agent) = self.assistant.agent.take() else {
            return;
        };
        let step = agent.ask(&text, &AssistantHost(self));
        self.assistant.agent = Some(agent);
        self.follow(step);
    }

    /// Answers the approval card.
    pub(in crate::app) fn assistant_decide(&mut self, allow: bool) {
        let always = self.assistant.always;
        self.assistant.always = false;
        let Some(mut agent) = self.assistant.agent.take() else {
            return;
        };
        let step = agent.decide(allow, always, &mut AssistantHost(self));
        self.assistant.agent = Some(agent);
        self.follow(step);
    }

    pub(in crate::app) fn assistant_stop(&mut self) {
        if let Some(agent) = self.assistant.agent.as_mut() {
            agent.stop();
        }
        self.assistant.inflight = None;
        self.assistant.retry = None;
    }

    pub(in crate::app) fn assistant_clear(&mut self) {
        if let Some(agent) = self.assistant.agent.as_mut() {
            agent.clear();
        }
        self.assistant.inflight = None;
        self.assistant.retry = None;
    }

    pub(in crate::app) fn assistant_compact(&mut self) {
        let Some(mut agent) = self.assistant.agent.take() else {
            return;
        };
        let step = agent.compact();
        self.assistant.agent = Some(agent);
        self.follow(step);
    }

    /// Lists the selected provider's models in the background.
    pub(in crate::app) fn assistant_list_models(&mut self) {
        let Some(config) = self.assistant.agent.as_ref().map(|a| a.config.clone()) else {
            return;
        };
        if !provider::allowed_url(&config.base_url) {
            self.assistant.models_error = Some("The address isn't allowed.".into());
            return;
        }
        let wake = self.ctx.clone();
        let mut request = to_request(provider::models_request(&config));
        request.limit = MODEL_LIST_LIMIT;
        let call = model_http::send(self.runtime.handle(), request, move || {
            wake.request_repaint()
        });
        self.assistant.models_error = None;
        self.assistant.models_call = Some((config.provider, call));
    }

    fn follow(&mut self, step: Step) {
        match step {
            Step::Send(req) => {
                let Some(generation) = self.assistant.agent.as_ref().map(Agent::generation) else {
                    return;
                };
                if !provider::allowed_url(&req.url) {
                    let Some(mut agent) = self.assistant.agent.take() else {
                        return;
                    };
                    let step = agent.receive(
                        generation,
                        Err(Error::from("The provider address must use https:// (http:// only for this computer or the local network).")),
                        &mut AssistantHost(self),
                    );
                    self.assistant.agent = Some(agent);
                    return self.follow(step);
                }
                let wake = self.ctx.clone();
                let call = model_http::send(self.runtime.handle(), to_request(req), move || {
                    wake.request_repaint()
                });
                self.assistant.inflight = Some((generation, call));
            }
            Step::Retry(after) => {
                let generation = self.assistant.agent.as_ref().map_or(0, Agent::generation);
                self.assistant.retry = Some((generation, Instant::now() + after));
                self.ctx.request_repaint_after(after);
            }
            Step::Confirm | Step::Done => {}
        }
        self.ctx.request_repaint();
    }

    /// Moves the assistant along: streamed pieces, finished requests, retries and queued
    /// questions. Runs every frame from `logic`.
    pub(in crate::app) fn poll_assistant(&mut self) {
        if let Some(question) = self.assistant.queued.take() {
            if self.assistant.busy() {
                self.assistant.queued = Some(question);
            } else {
                self.assistant_send(question);
            }
        }
        if let Some((generation, call)) = &self.assistant.inflight {
            let generation = *generation;
            let chunks = call.take_chunks();
            if !chunks.is_empty()
                && let Some(agent) = self.assistant.agent.as_mut()
            {
                agent.stream(generation, &chunks);
            }
            if let Some(result) = call.take_result() {
                self.assistant.inflight = None;
                if let Some(mut agent) = self.assistant.agent.take() {
                    let step =
                        agent.receive(generation, to_response(result), &mut AssistantHost(self));
                    self.assistant.agent = Some(agent);
                    self.follow(step);
                }
            }
        }
        if let Some((generation, at)) = self.assistant.retry {
            let now = Instant::now();
            if now >= at {
                self.assistant.retry = None;
                if let Some(mut agent) = self.assistant.agent.take() {
                    let step = agent.retry(generation);
                    self.assistant.agent = Some(agent);
                    self.follow(step);
                }
            } else {
                self.ctx.request_repaint_after(at - now);
            }
        }
        if let Some((provider_id, call)) = &self.assistant.models_call
            && let Some(result) = call.take_result()
        {
            let api =
                provider::preset(provider_id).map_or(globecraft_assistant::Api::OpenAi, |p| p.api);
            self.assistant.models_call = None;
            match to_response(result) {
                Ok(resp) => match provider::parse_models(api, &resp) {
                    Ok(models) if models.is_empty() => {
                        self.assistant.models_error = Some("The provider listed no models.".into())
                    }
                    Ok(models) => self.assistant.models = models,
                    Err(e) => self.assistant.models_error = Some(e),
                },
                Err(e) => self.assistant.models_error = Some(e.to_string()),
            }
        }
        // Keep frames coming while a reply streams so the preview grows smoothly.
        if self.assistant.inflight.is_some() {
            self.ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}
