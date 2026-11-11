//! The tools the model calls, and what they do through the [`Host`].
//!
//! Every GlobeCraft action is a command (see `globecraft_core::commands`), so a handful of tools
//! reach all of them: the model finds a command by searching, then runs it by id. Commands that
//! read or write files, or discard work, wait for the user's approval ([`gate`]); commands that
//! only open interface surfaces are not offered at all.
//!
//! The budgeting helpers are adapted from RustGIS (`crates/assistant/src/tools.rs`, Apache-2.0).

use globecraft_core::commands::{self, CommandSpec};
use globecraft_core::language::Lang;
use serde_json::{Value, json};

use crate::provider::{ToolCall, ToolDef};

/// What the assistant acts on: the application, which runs commands by id.
pub trait Host {
    /// Runs a command; `Err` is reported to the model as a failure.
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String>;
    /// A short description of what is on screen, sent with the user's message when it changed.
    fn context(&self) -> String;
}

/// Default per-tool token estimate.
pub const DEFAULT_OUTPUT_TOKENS: usize = 10_000;
/// Absolute per-tool ceiling.
pub const MAX_OUTPUT_TOKENS: usize = 24_000;
/// A requested token estimate as a clamped byte budget.
pub fn output_bytes(tokens: usize) -> usize {
    tokens.clamp(1, MAX_OUTPUT_TOKENS) * 4
}

/// The tools the model is given.
pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "describe_view",
            description: "What is on screen now: the camera (latitude, longitude, altitude, heading), the world (Earth, Moon or a custom planet), basemap, visible layers, units, the ruler's measurement, the number of places and the latest search results.",
            schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "search_commands",
            description: "Search GlobeCraft commands (camera, search, layers, basemaps, placemarks, ruler, worlds, settings…) by words. Returns each command's id, label, description and JSON parameters.",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words to look for, e.g. \"fly\", \"terrain\", \"placemark\""},
                    "limit": {"type": "integer", "description": "At most this many (default 12)"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "run_command",
            description: "Run an GlobeCraft command by id with JSON parameters (see search_commands for ids and parameters). Commands that read or write files, or discard work, ask the user first.",
            schema: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Command id, e.g. camera.flyTo"},
                    "params": {"type": "object", "description": "The command's parameters"}
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "list_places",
            description: "Every placemark and folder in My Places, with names, descriptions, coordinates and visibility.",
            schema: json!({"type": "object", "properties": {}}),
        },
    ]
}

/// The command a call runs and its parameters, if it runs one.
pub fn target(call: &ToolCall) -> Option<(String, Value)> {
    match call.name.as_str() {
        "run_command" => Some((
            str_arg(&call.args, "command").unwrap_or_default(),
            obj_arg(&call.args, "params"),
        )),
        _ => None,
    }
}

/// Whether a call needs the user's approval, and why (English, for the approval card).
pub fn gate(call: &ToolCall) -> Option<String> {
    let (id, params) = target(call)?;
    let spec = commands::find(&id)?;
    // A network command that names no address uses a built-in source (a library preset).
    if spec.reach == commands::Reach::Network && params.get("url").is_none() {
        return None;
    }
    spec.approval_reason().map(str::to_string)
}

/// What "allow for this conversation" remembers: for file access the command with its exact
/// parameters, for a web address the command and the address's host. Destructive commands are
/// asked about every time.
pub fn approval_key(call: &ToolCall) -> Option<String> {
    let (id, params) = target(call)?;
    let spec = commands::find(&id)?;
    match spec.reach {
        commands::Reach::Files => {
            let params = params_arg(&call.args).ok()?;
            Some(format!("{id} {params}"))
        }
        commands::Reach::Network => {
            let host = url_host(params.get("url")?.as_str()?)?;
            Some(format!("{id} host {host}"))
        }
        _ => None,
    }
}

/// The lower-case host of an HTTP(S) address (no port, no user, no brackets), or `None`.
fn url_host(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = &rest[..rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len())];
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        &bracketed[..bracketed.find(']')?]
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    let host = host.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_owned())
}

/// Whether a host is this computer or the local network: names such as `localhost` and
/// `*.local`, private, loopback and link-local addresses, and any numeric host written in a
/// form other than the four plain decimal parts (`2130706433`, `0x7f.1`) that a URL
/// parser would still read as an IPv4 address.
fn is_local_host(host: &str) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    let private_v4 = |ip: Ipv4Addr| {
        let [a, b, ..] = ip.octets();
        ip.is_private()
            || ip.is_loopback()
            || ip.is_link_local()
            || ip.is_unspecified()
            || ip.is_broadcast()
            // Shared address space (carrier-grade NAT), 100.64.0.0/10.
            || (a == 100 && (64..128).contains(&b))
            || a == 0
    };
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => private_v4(ip),
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                // Unique local fc00::/7 and link-local fe80::/10.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || ip.to_ipv4_mapped().is_some_and(private_v4)
        }
        Err(_) => {
            // A last label of digits or 0x-hex makes the host an IPv4 address to URL parsers.
            let last = host.rsplit('.').next().unwrap_or(host);
            !last.is_empty()
                && (last.bytes().all(|b| b.is_ascii_digit())
                    || last
                        .strip_prefix("0x")
                        .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit())))
        }
    }
}

/// A one-line description of a call for the transcript and the approval card.
pub fn summary(call: &ToolCall) -> String {
    let short = |v: Value| {
        let t = v.to_string();
        if t.chars().count() > 300 {
            format!("{}…", t.chars().take(300).collect::<String>())
        } else {
            t
        }
    };
    match call.name.as_str() {
        "run_command" => format!(
            "{} {}",
            str_arg(&call.args, "command").unwrap_or_default(),
            short(obj_arg(&call.args, "params"))
        ),
        other => format!("{other} {}", short(call.args.clone())),
    }
}

/// Runs a call with a per-result token estimate; `Err` is a result the model is told failed.
pub fn run(host: &mut dyn Host, call: &ToolCall, tokens: usize) -> Result<String, String> {
    let a = &call.args;
    let v = match call.name.as_str() {
        "describe_view" => host.run("info.view", &json!({}))?,
        "list_places" => host.run("info.places", &json!({}))?,
        "search_commands" => search_commands(a)?,
        "run_command" => {
            let id = str_arg(a, "command").ok_or("command is required")?;
            let spec = commands::find(&id)
                .ok_or_else(|| format!("unknown command '{id}'; find ids with search_commands"))?;
            if !spec.assistant_may_run() {
                return Err(format!(
                    "'{id}' only opens part of the interface; the assistant can't use it"
                ));
            }
            let params = params_arg(a)?;
            if spec.reach == commands::Reach::Network
                && let Some(url) = params.get("url")
            {
                let host = url
                    .as_str()
                    .and_then(url_host)
                    .ok_or("url must be an https:// or http:// address")?;
                if is_local_host(&host) {
                    return Err(format!(
                        "'{host}' is this computer or the local network; the assistant may only add public web addresses"
                    ));
                }
            }
            host.run(&id, &params)?
        }
        other => return Err(format!("unknown tool '{other}'")),
    };
    Ok(bounded_json(&v, tokens))
}

/// The English label and help of a command, for the model.
fn describe(spec: &CommandSpec) -> Value {
    json!({
        "id": spec.id,
        "label": Lang::En.text(spec.label),
        "description": Lang::En.text(spec.help),
        "params": serde_json::from_str::<Value>(spec.params).unwrap_or(Value::Null),
        "needs_approval": spec.approval_reason().is_some(),
    })
}

fn search_commands(a: &Value) -> Result<Value, String> {
    let query = str_arg(a, "query").unwrap_or_default().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Err("query is required".into());
    }
    let limit = limit_arg(a, 12, 50);
    let mut scored: Vec<(usize, &CommandSpec)> = commands::COMMANDS
        .iter()
        .filter(|spec| spec.assistant_may_run())
        .filter_map(|spec| {
            let haystack = format!(
                "{} {} {} {}",
                spec.id,
                Lang::En.text(spec.label),
                Lang::En.text(spec.help),
                spec.params
            )
            .to_lowercase();
            let id = spec.id.to_lowercase();
            let score: usize = words
                .iter()
                .map(|w| usize::from(haystack.contains(w)) + 2 * usize::from(id.contains(w)))
                .sum();
            (score > 0).then_some((score, spec))
        })
        .collect();
    // Stable: equal scores keep catalog (ribbon) order.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let hits: Vec<Value> = scored
        .into_iter()
        .take(limit)
        .map(|(_, spec)| describe(spec))
        .collect();
    Ok(json!({"count": hits.len(), "commands": hits}))
}

/// Serializes complete JSON. Oversized results become an explicit text preview envelope.
pub fn bounded_json(value: &Value, tokens: usize) -> String {
    let text = value.to_string();
    let max = output_bytes(tokens);
    if text.len() <= max {
        return text;
    }
    let chars: Vec<char> = text.chars().collect();
    let envelope = |keep: usize| {
        let head: String = chars.iter().take(keep / 2).collect();
        let tail: String = chars
            .iter()
            .skip(chars.len().saturating_sub(keep - keep / 2))
            .collect();
        json!({"truncated": true, "original_bytes": text.len(),
            "output": format!("{head}\n… output truncated …\n{tail}")})
        .to_string()
    };
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if envelope(mid).len() <= max {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let result = envelope(low);
    if result.len() <= max {
        result
    } else {
        json!({"truncated": true}).to_string()
    }
}

/// `text`, cut to `max` characters with a note when longer.
pub fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!(
        "{}… (cut: {} characters in all)",
        text.chars().take(max).collect::<String>(),
        text.chars().count()
    )
}

fn str_arg(a: &Value, k: &str) -> Option<String> {
    match a.get(k)? {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The `params` a command runs with: an object, or JSON text of one (some models send that).
/// Anything else is an error rather than a silent run with defaults.
fn params_arg(a: &Value) -> Result<Value, String> {
    match a.get("params") {
        None | Some(Value::Null) => Ok(json!({})),
        Some(v @ Value::Object(_)) => Ok(v.clone()),
        Some(Value::String(t)) if t.trim().is_empty() => Ok(json!({})),
        Some(Value::String(t)) => match serde_json::from_str::<Value>(t) {
            Ok(v @ Value::Object(_)) => Ok(v),
            Ok(_) => Err("params must be a JSON object".into()),
            Err(e) => Err(format!("params is not valid JSON: {e}")),
        },
        Some(_) => Err("params must be a JSON object".into()),
    }
}

/// An object parameter, leniently, for display and approval.
fn obj_arg(a: &Value, k: &str) -> Value {
    match a.get(k) {
        Some(Value::String(t)) if !t.trim().is_empty() => {
            serde_json::from_str(t).unwrap_or(Value::Null)
        }
        Some(v @ Value::Object(_)) => v.clone(),
        _ => json!({}),
    }
}

fn limit_arg(a: &Value, default: usize, max: usize) -> usize {
    a.get("limit")
        .and_then(Value::as_u64)
        .map_or(default, |n| usize::try_from(n).unwrap_or(max))
        .clamp(1, max)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: name.into(),
            args,
        }
    }

    #[test]
    fn search_finds_commands_by_words_and_hides_interface_ones() {
        let r = search_commands(&json!({"query": "fly"})).unwrap();
        let ids: Vec<&str> = r["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.first(), Some(&"camera.flyTo"));
        let r = search_commands(&json!({"query": "settings pane"})).unwrap();
        assert!(
            r["commands"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| !c["id"].as_str().unwrap().starts_with("ui.pane"))
        );
        assert!(search_commands(&json!({"query": " "})).is_err());
    }

    #[test]
    fn files_and_destructive_commands_are_gated() {
        let shot = call("run_command", json!({"command": "file.screenshot"}));
        assert!(gate(&shot).is_some());
        assert!(approval_key(&shot).is_some());
        let clear = call("run_command", json!({"command": "places.clear"}));
        assert!(gate(&clear).is_some());
        assert!(approval_key(&clear).is_none(), "asked every time");
        let fly = call(
            "run_command",
            json!({"command": "camera.flyTo", "params": {"lat": 1, "lon": 2}}),
        );
        assert!(gate(&fly).is_none());
    }

    #[test]
    fn web_addresses_are_gated_by_host_and_library_presets_are_not() {
        let preset = call(
            "run_command",
            json!({"command": "data.add", "params": {"preset": "earthquakes"}}),
        );
        assert!(gate(&preset).is_none());
        let web = |url: &str| {
            call(
                "run_command",
                json!({"command": "data.add", "params": {"url": url, "kind": "features"}}),
            )
        };
        let first = web("https://data.example/a.geojson");
        assert!(gate(&first).is_some());
        // Allowing a host covers other files on it, not other hosts.
        assert_eq!(
            approval_key(&first),
            approval_key(&web("https://DATA.example:443/b.csv"))
        );
        assert_ne!(
            approval_key(&first),
            approval_key(&web("https://other.example/a.geojson"))
        );
        assert_eq!(
            url_host("https://user@evil.example/x").as_deref(),
            Some("evil.example")
        );
        assert_eq!(url_host("http://[::1]:8080/").as_deref(), Some("::1"));
        assert_eq!(url_host("file:///etc/passwd"), None);
    }

    #[test]
    fn local_and_private_hosts_are_recognised() {
        for host in [
            "localhost",
            "printer.local",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2130706433",
            "0x7f.1",
            "127.1",
        ] {
            assert!(is_local_host(host), "{host}");
        }
        for host in [
            "example.org",
            "8.8.8.8",
            "2001:4860::8888",
            "cafe.de",
            "1.example",
        ] {
            assert!(!is_local_host(host), "{host}");
        }
    }

    #[test]
    fn params_must_be_objects() {
        assert!(params_arg(&json!({"params": "[1]"})).is_err());
        assert_eq!(
            params_arg(&json!({"params": "{\"a\":1}"})).unwrap(),
            json!({"a": 1})
        );
        assert_eq!(params_arg(&json!({})).unwrap(), json!({}));
    }

    #[test]
    fn oversized_results_are_bounded_and_valid_json() {
        let big = json!({"rows": "x".repeat(200_000)});
        let out = bounded_json(&big, 100);
        assert!(out.len() <= output_bytes(100));
        assert!(serde_json::from_str::<Value>(&out).is_ok());
    }
}
