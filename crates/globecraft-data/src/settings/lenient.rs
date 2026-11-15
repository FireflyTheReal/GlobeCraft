//! Reading settings through serde, one field at a time.
//!
//! A settings file is written by GlobeCraft, by older and newer versions of it, and
//! sometimes by hand. Whatever it holds, every value that can be used is used and
//! every other setting keeps its value: nothing in a file makes the rest unreadable.
//!
//! A section is a struct that derives `Serialize` and `Deserialize`; [`apply`] reads
//! its plain fields by name and type. A new setting is a field and its default.
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Reads the plain fields of `current` that `file` holds: a boolean, number, text or
/// list (or `null`, for a field that may be absent) under the field's name. A value of
/// the wrong type, or one the field's type refuses (an unknown name of an enum), leaves
/// that field as it was; so do keys `T` does not have.
///
/// Sections and tables are not read here: a field saved as an object, and any object
/// in the file, is left to the `apply` of its own type, which the struct holding it
/// calls. `own_rules` names fields whose owner reads them
/// itself, because a type cannot say the rule (two numbers that are valid only
/// together, a list whose entries are filtered one by one).
pub(super) fn apply<T>(current: &mut T, file: &Value, own_rules: &[&str])
where
    T: Serialize + DeserializeOwned,
{
    let (Ok(Value::Object(base)), Some(file)) = (serde_json::to_value(&*current), file.as_object())
    else {
        return;
    };
    let offered: Vec<(&String, &Value)> = file
        .iter()
        .filter(|(key, value)| {
            base.get(*key).is_some_and(|saved| !saved.is_object())
                && !value.is_object()
                && !own_rules.contains(&key.as_str())
        })
        .collect();
    if offered.is_empty() {
        return;
    }
    // Nearly always every value is good: one read.
    let mut all = base.clone();
    for (key, value) in &offered {
        all.insert((*key).clone(), (*value).clone());
    }
    if let Ok(read) = serde_json::from_value(Value::Object(all)) {
        *current = read;
        return;
    }
    // Otherwise one at a time, keeping each that reads.
    debug_assert!(
        serde_json::from_value::<T>(Value::Object(base.clone())).is_ok(),
        "settings must read back what they save"
    );
    let mut accepted = base;
    for (key, value) in offered {
        let before = accepted.insert(key.clone(), value.clone());
        match serde_json::from_value(Value::Object(accepted.clone())) {
            Ok(read) => *current = read,
            Err(_) => {
                if let Some(before) = before {
                    accepted.insert(key.clone(), before);
                }
            }
        }
    }
}

/// For a field saved under a short name (`#[serde(deserialize_with = …)]`): the text,
/// turned back by `from_code`, or an error for a name it does not know.
pub(super) fn coded<'de, D, T>(
    deserializer: D,
    from_code: impl FnOnce(&str) -> Option<T>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    let code = String::deserialize(deserializer)?;
    from_code(&code).ok_or_else(|| serde::de::Error::custom(format!("unknown name {code:?}")))
}

/// For a field whose type reads itself with an `apply` (a table, a list with rules):
/// its default with the saved value applied.
pub(super) fn applied<'de, D, T>(
    deserializer: D,
    apply: impl FnOnce(&mut T, &Value),
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default,
{
    let value = Value::deserialize(deserializer)?;
    let mut read = T::default();
    apply(&mut read, &value);
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Mode {
        #[default]
        Plain,
        Fancy,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Inner {
        depth: u8,
    }
    impl Default for Inner {
        fn default() -> Self {
            Self { depth: 3 }
        }
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Section {
        on: bool,
        size: f32,
        name: String,
        mode: Mode,
        tags: Vec<String>,
        limit: Option<u32>,
        low: f64,
        high: f64,
        inner: Inner,
    }
    impl Default for Section {
        fn default() -> Self {
            Self {
                on: true,
                size: 1.5,
                name: "default".into(),
                mode: Mode::Plain,
                tags: vec!["a".into()],
                limit: Some(7),
                low: 1.0,
                high: 2.0,
                inner: Inner::default(),
            }
        }
    }

    fn read(file: Value) -> Section {
        let mut section = Section::default();
        apply(&mut section, &file, &[]);
        section
    }

    #[test]
    fn good_values_are_read_and_the_rest_keeps_its_default() {
        let section = read(serde_json::json!({
            "on": false, "size": 2, "name": "x", "mode": "fancy", "tags": ["b", "c"],
            "limit": null, "unknown": 1,
        }));
        assert_eq!(
            section,
            Section {
                on: false,
                size: 2.0,
                name: "x".into(),
                mode: Mode::Fancy,
                tags: vec!["b".into(), "c".into()],
                limit: None,
                ..Section::default()
            }
        );
        assert_eq!(read(serde_json::json!({})), Section::default());
        assert_eq!(read(serde_json::json!([1, 2])), Section::default());
        assert_eq!(read(Value::Null), Section::default());
    }

    #[test]
    fn a_bad_value_costs_only_its_own_field() {
        // `on` is not a boolean, `mode` not a mode, `tags` holds a number, `size` is
        // null: each keeps its default (true, not `bool::default()`), the others read.
        let section = read(serde_json::json!({
            "on": "yes", "size": null, "name": "kept", "mode": "baroque", "tags": ["b", 3],
            "limit": 9, "low": 0.5,
        }));
        assert_eq!(
            section,
            Section {
                name: "kept".into(),
                limit: Some(9),
                low: 0.5,
                ..Section::default()
            }
        );
    }

    #[test]
    fn values_apply_onto_what_is_there_not_onto_the_defaults() {
        let mut section = Section::default();
        apply(
            &mut section,
            &serde_json::json!({"name": "first", "on": false}),
            &[],
        );
        apply(
            &mut section,
            &serde_json::json!({"size": 4.0, "on": 3}),
            &[],
        );
        assert_eq!(
            (section.name.as_str(), section.on, section.size),
            ("first", false, 4.0)
        );
    }

    #[test]
    fn objects_and_fields_with_their_own_rules_are_left_to_their_owner() {
        let mut section = Section::default();
        apply(
            &mut section,
            &serde_json::json!({"inner": {"depth": 9}, "low": 5.0, "high": 6.0, "on": false}),
            &["low", "high"],
        );
        assert_eq!(
            section,
            Section {
                on: false,
                ..Section::default()
            }
        );
        // Not an object where a section is saved: still its owner's to refuse.
        apply(
            &mut section,
            &serde_json::json!({"inner": 42, "name": "n"}),
            &[],
        );
        assert_eq!(section.inner, Inner::default());
        assert_eq!(section.name, "n");
    }

    #[test]
    fn coded_and_applied_fields_read_back_what_was_saved() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Holder {
            #[serde(deserialize_with = "mode")]
            mode: Mode,
            #[serde(deserialize_with = "doubled")]
            numbers: Vec<u8>,
        }
        fn mode<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Mode, D::Error> {
            coded(deserializer, |code| (code == "f").then_some(Mode::Fancy))
        }
        fn doubled<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
            applied(deserializer, |numbers: &mut Vec<u8>, value| {
                numbers.extend(
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| n.as_u64().and_then(|n| u8::try_from(n * 2).ok())),
                );
            })
        }
        let holder: Holder =
            serde_json::from_value(serde_json::json!({"mode": "f", "numbers": [1, "x", 200, 4]}))
                .unwrap();
        assert_eq!(
            holder,
            Holder {
                mode: Mode::Fancy,
                numbers: vec![2, 8]
            }
        );
        assert!(
            serde_json::from_value::<Holder>(serde_json::json!({"mode": "g", "numbers": []}))
                .is_err()
        );
    }
}
