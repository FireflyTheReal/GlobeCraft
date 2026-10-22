//! The AI assistant: a chat agent that answers questions about the globe and acts on it by
//! running GlobeCraft commands, through a model of the user's choosing (Anthropic, OpenAI,
//! Gemini, OpenRouter, Ollama or any OpenAI-compatible server).
//!
//! - [`provider`]: the wire formats, as plain request/response data.
//! - [`tools`]: what the model may call, and which calls need the user's approval.
//! - [`agent`]: the conversation loop, without I/O, so the desktop and the tests drive the
//!   same code. The desktop sends requests on its service runtime.
//!
//! The design follows RustGIS's assistant (Apache-2.0); see CREDITS.md.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod agent;
pub mod compact;
pub mod error;
pub mod provider;
#[cfg(test)]
mod tests;
pub mod tools;

pub use agent::{Agent, Entry, Step};
pub use error::{Error, TransportFailure};
pub use provider::{Api, Config, HttpRequest, HttpResponse, Note, PRESETS, Preset};
pub use tools::Host;
