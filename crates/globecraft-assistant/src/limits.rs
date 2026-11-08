//! Limits on the tool calls one user message may set off, so a model stuck in a loop (running
//! the same command again and again, or calling tools without end) stops with a note instead of
//! acting on the globe until the step limit ([`crate::agent::MAX_STEPS`]) runs out.

use crate::provider::ToolCall;
use serde_json::Value;

/// Tool calls run for one user message, over every reply it sets off. A tour of a few dozen
/// placemarks fits; a loop doesn't go on for long.
pub const MAX_CALLS_PER_MESSAGE: u32 = 64;

/// Identical calls (same tool, same arguments) run in a row before the next one is refused.
/// A few repeats are legitimate ("zoom in more"); more mean the model isn't seeing its results.
pub const MAX_IDENTICAL_CALLS: u32 = 4;

/// Why a call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Limit {
    /// [`MAX_CALLS_PER_MESSAGE`] calls already ran.
    Calls,
    /// The call is identical to the [`MAX_IDENTICAL_CALLS`] before it.
    Repeats,
}

/// Counts the calls of one user message.
#[derive(Debug, Default)]
pub(crate) struct CallLimiter {
    calls: u32,
    /// The last call admitted (tool and arguments) and how many times in a row it came.
    last: Option<(String, Value)>,
    repeats: u32,
}

impl CallLimiter {
    /// Starts counting afresh (a new message from the user, "continue" included).
    pub(crate) fn reset(&mut self) {
        *self = CallLimiter::default();
    }

    /// Counts `call` if it may run; a refused call isn't counted.
    pub(crate) fn admit(&mut self, call: &ToolCall) -> Result<(), Limit> {
        if self.calls >= MAX_CALLS_PER_MESSAGE {
            return Err(Limit::Calls);
        }
        let same = self
            .last
            .as_ref()
            .is_some_and(|(name, args)| *name == call.name && *args == call.args);
        if same {
            if self.repeats >= MAX_IDENTICAL_CALLS {
                return Err(Limit::Repeats);
            }
            self.repeats += 1;
        } else {
            self.last = Some((call.name.clone(), call.args.clone()));
            self.repeats = 1;
        }
        self.calls += 1;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(command: &str, params: Value) -> ToolCall {
        ToolCall {
            id: String::new(),
            name: "run_command".into(),
            args: json!({"command": command, "params": params}),
        }
    }

    #[test]
    fn identical_calls_in_a_row_are_capped() {
        let mut limiter = CallLimiter::default();
        let zoom = call("camera.zoomIn", json!({}));
        for _ in 0..MAX_IDENTICAL_CALLS {
            assert_eq!(limiter.admit(&zoom), Ok(()));
        }
        assert_eq!(limiter.admit(&zoom), Err(Limit::Repeats));
        // Other parameters make another call, which starts a new run.
        assert_eq!(
            limiter.admit(&call("camera.flyTo", json!({"lat": 1, "lon": 2}))),
            Ok(())
        );
        assert_eq!(limiter.admit(&zoom), Ok(()));
        // A new message starts afresh.
        limiter.reset();
        for _ in 0..MAX_IDENTICAL_CALLS {
            assert_eq!(limiter.admit(&zoom), Ok(()));
        }
    }

    #[test]
    fn calls_per_message_are_capped() {
        let mut limiter = CallLimiter::default();
        for i in 0..MAX_CALLS_PER_MESSAGE {
            assert_eq!(
                limiter.admit(&call("places.add", json!({"lat": i, "lon": 0}))),
                Ok(())
            );
        }
        assert_eq!(
            limiter.admit(&call("places.add", json!({"lat": -1, "lon": 0}))),
            Err(Limit::Calls)
        );
        limiter.reset();
        assert_eq!(limiter.admit(&call("info.view", json!({}))), Ok(()));
    }
}
