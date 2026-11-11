//! `docs/COMMANDS.md` is generated from the command catalog and the English labels, so
//! the reference cannot drift from the code. When this test fails, regenerate the file:
//!
//! ```sh
//! UPDATE_COMMAND_REFERENCE=1 cargo test -p globecraft-core --test command_reference
//! ```
use globecraft_core::{
    commands::{COMMANDS, CommandSpec, Reach, Tab},
    language::Lang,
};
use serde_json::Value;
use std::fmt::Write as _;

const REFERENCE_PATH: &str = "../../docs/COMMANDS.md";
const UPDATE_VARIABLE: &str = "UPDATE_COMMAND_REFERENCE";

#[test]
fn command_reference_matches_the_catalog() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REFERENCE_PATH);
    let generated = reference();
    if std::env::var_os(UPDATE_VARIABLE).is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        stored == generated,
        "docs/COMMANDS.md is out of date; run `{UPDATE_VARIABLE}=1 cargo test -p globecraft-core --test command_reference`"
    );
}

fn reference() -> String {
    let lang = Lang::En;
    let mut out = String::from(
        "# Commands\n\n\
         Generated from `crates/globecraft-core/src/commands.rs` and the English labels by\n\
         `crates/globecraft-core/tests/command_reference.rs`; do not edit by hand. Every\n\
         command can be run from Command Search, and the assistant can run those marked so.\n\
         Parameters are a JSON object; none is required unless marked.\n",
    );
    for tab in Tab::ALL {
        write!(
            out,
            "\n## {}\n\n| Command | Name | What it does | Parameters | Assistant |\n| --- | --- | --- | --- | --- |\n",
            lang.text(tab.label_key())
        ).unwrap();
        for spec in COMMANDS.iter().filter(|spec| spec.tab == tab) {
            writeln!(
                out,
                "| `{}` | {} | {} | {} | {} |",
                spec.id,
                cell(lang.text(spec.label)),
                cell(lang.text(spec.help)),
                parameters(spec),
                assistant(spec),
            )
            .unwrap();
        }
    }
    out
}

fn assistant(spec: &CommandSpec) -> String {
    match spec.reach {
        Reach::Interface => return "no".to_owned(),
        // Built-in sources run unasked; only addresses in the parameters need approval.
        Reach::Network => return "asks first when it names a web address".to_owned(),
        Reach::View | Reach::Files | Reach::Destructive => {}
    }
    match spec.approval_reason() {
        Some(reason) => format!("asks first: {}", cell(reason)),
        None => "yes".to_owned(),
    }
}

/// One line per parameter: name, type, whether required, allowed values and description.
fn parameters(spec: &CommandSpec) -> String {
    let schema: Value = serde_json::from_str(spec.params).unwrap();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let Some(properties) = schema["properties"]
        .as_object()
        .filter(|map| !map.is_empty())
    else {
        return "—".to_owned();
    };
    properties
        .iter()
        .map(|(name, property)| {
            let mut line = format!("`{name}`");
            if let Some(kind) = property["type"].as_str() {
                write!(line, " ({kind})").unwrap();
            }
            if required.contains(&name.as_str()) {
                line.push_str(", required");
            }
            if let Some(values) = property["enum"].as_array() {
                let values: Vec<String> = values
                    .iter()
                    .map(|value| match value {
                        Value::String(text) => format!("`{text}`"),
                        other => format!("`{other}`"),
                    })
                    .collect();
                write!(line, ": one of {}", values.join(", ")).unwrap();
            }
            if let Some(description) = property["description"].as_str() {
                write!(line, ": {}", cell(description)).unwrap();
            }
            line
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

/// Text safe inside a Markdown table cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}
