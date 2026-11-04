//! What a point's properties say beyond its name (roadmap 4.I): the value its marker
//! is sized and ranked by (magnitude, population…), when it happened, and a few other
//! properties for its hover card.
use std::borrow::Cow;

/// Property names tried for the value when the layer names none, in order.
const VALUE_FIELDS: [&str; 10] = [
    "mag",
    "magnitude",
    "value",
    "size",
    "population",
    "pop",
    "count",
    "capacity_mw",
    "capacity",
    "amount",
];
/// Property names tried for the time, in order.
const TIME_FIELDS: [&str; 6] = ["time", "timestamp", "datetime", "date", "updated", "when"];
/// Properties not repeated in the hover details.
const NAME_FIELDS: [&str; 7] = ["name", "Name", "NAME", "title", "Title", "place", "label"];
/// At most this many details, each at most this long, per point.
const MAX_DETAILS: usize = 4;
const MAX_DETAIL_CHARS: usize = 60;
/// Numbers above this are milliseconds since 1970, below it seconds.
const MILLISECOND_EPOCH_ABOVE: f64 = 1e11;

/// How a layer reads its points' properties: the value property (given, or found on
/// the first point that has one of `VALUE_FIELDS`).
#[derive(Debug, Default)]
pub(super) struct PropertyReader {
    value_field: Option<String>,
    /// The value property was given or found, so no more searching.
    settled: bool,
}

/// What one point's properties hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct PointProperties {
    pub value: Option<f64>,
    pub time_ms: Option<i64>,
    pub details: String,
}

impl PropertyReader {
    pub(super) fn new(value_field: Option<&str>) -> Self {
        let value_field = value_field.map(str::trim).filter(|f| !f.is_empty());
        Self {
            value_field: value_field.map(str::to_owned),
            settled: value_field.is_some(),
        }
    }

    /// The property the values come from, once known.
    pub(super) fn value_field(&self) -> Option<&str> {
        self.value_field.as_deref()
    }

    /// Reads one point's properties, given as (name, value) pairs where `value`
    /// is the text of a scalar (numbers as written).
    pub(super) fn read<'a>(
        &mut self,
        properties: impl Iterator<Item = (&'a str, Scalar<'a>)> + Clone,
    ) -> PointProperties {
        if !self.settled
            && let Some(found) = VALUE_FIELDS.iter().find(|field| {
                properties
                    .clone()
                    .any(|(key, value)| key.eq_ignore_ascii_case(field) && value.number().is_some())
            })
        {
            self.value_field = Some((*found).to_owned());
            self.settled = true;
        }
        let field = self.value_field.as_deref();
        let value = field.and_then(|field| {
            properties
                .clone()
                .find(|(key, _)| key.eq_ignore_ascii_case(field))
                .and_then(|(_, value)| value.number())
        });
        let time = TIME_FIELDS.iter().find_map(|field| {
            properties
                .clone()
                .find(|(key, _)| key.eq_ignore_ascii_case(field))
                .and_then(|(key, value)| Some((key, value.time_ms()?)))
        });
        let time_ms = time.map(|(_, ms)| ms);
        let mut details = String::new();
        let mut count = 0;
        for (key, value) in properties {
            if count == MAX_DETAILS {
                break;
            }
            let text = value.text();
            if text.is_empty()
                || text.chars().count() > MAX_DETAIL_CHARS
                || NAME_FIELDS.contains(&key)
                || field.is_some_and(|f| key.eq_ignore_ascii_case(f))
                || time.is_some_and(|(time_key, _)| time_key == key)
                || text.starts_with("http")
            {
                continue;
            }
            if !details.is_empty() {
                details.push('\n');
            }
            details.push_str(key);
            details.push_str(": ");
            details.push_str(&text);
            count += 1;
        }
        PointProperties {
            value,
            time_ms,
            details,
        }
    }
}

/// A property's scalar value: text (a CSV field, a JSON string) or a JSON number.
#[derive(Clone, Copy, Debug)]
pub(super) enum Scalar<'a> {
    Text(&'a str),
    Number(f64),
}

impl<'a> Scalar<'a> {
    fn text(self) -> Cow<'a, str> {
        match self {
            Self::Text(text) => Cow::Borrowed(text.trim()),
            Self::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
                Cow::Owned(format!("{}", n as i64))
            }
            Self::Number(n) => Cow::Owned(n.to_string()),
        }
    }
    fn number(self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(n),
            Self::Text(text) => text.trim().replace(',', ".").parse::<f64>().ok(),
        }
        .filter(|n| n.is_finite())
    }
    fn time_ms(self) -> Option<i64> {
        match self.number() {
            Some(n) if n >= MILLISECOND_EPOCH_ABOVE => Some(n as i64),
            Some(n) => Some((n * 1000.0) as i64),
            None => iso_time_ms(&self.text()),
        }
    }
}

/// Milliseconds since 1970 of an ISO 8601 date or date and time.
fn iso_time_ms(text: &str) -> Option<i64> {
    globecraft_core::time::parse_iso(text).map(|secs| (secs * 1000.0).round() as i64)
}

/// `2026-11-04 09:38 UTC` for milliseconds since 1970.
pub fn format_time_utc(ms: i64) -> String {
    globecraft_core::time::format_utc(ms as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props<'a>(
        pairs: &'a [(&'a str, Scalar<'a>)],
    ) -> impl Iterator<Item = (&'a str, Scalar<'a>)> + Clone {
        pairs.iter().copied()
    }

    #[test]
    fn the_value_field_is_found_once_and_kept() {
        let mut reader = PropertyReader::new(None);
        let first = [
            ("title", Scalar::Text("M 4.6 - Japan")),
            ("mag", Scalar::Number(4.6)),
            ("time", Scalar::Number(1_793_700_000_000.0)),
            ("tsunami", Scalar::Number(0.0)),
        ];
        let read = reader.read(props(&first));
        assert_eq!(reader.value_field(), Some("mag"));
        assert_eq!(read.value, Some(4.6));
        assert_eq!(read.time_ms, Some(1_793_700_000_000));
        // Name, value and time are not repeated in the details.
        assert_eq!(read.details, "tsunami: 0");
        // A later point with a population keeps the layer on magnitude.
        let second = [("population", Scalar::Number(5.0))];
        assert_eq!(reader.read(props(&second)).value, None);
    }

    #[test]
    fn a_given_value_field_wins_and_times_parse_in_every_form() {
        let mut reader = PropertyReader::new(Some("depth"));
        let pairs = [
            ("depth", Scalar::Text("12,5")),
            ("mag", Scalar::Number(3.0)),
        ];
        assert_eq!(reader.read(props(&pairs)).value, Some(12.5));
        assert_eq!(iso_time_ms("1970-01-02"), Some(86_400_000));
        assert_eq!(iso_time_ms("2026-11-04T09:38:00Z"), Some(1_793_785_080_000));
        assert_eq!(
            iso_time_ms("2026-11-04 10:38+01:00"),
            Some(1_793_785_080_000)
        );
        assert_eq!(
            iso_time_ms("2026-11-04T09:38:00.5"),
            Some(1_793_785_080_500)
        );
        assert_eq!(iso_time_ms("yesterday"), None);
        assert_eq!(iso_time_ms("2026-13-01"), None);
        assert_eq!(
            Scalar::Text("1793700000").time_ms(),
            Some(1_793_700_000_000)
        );
        assert_eq!(format_time_utc(1_793_785_080_000), "2026-11-04 09:38 UTC");
        assert_eq!(format_time_utc(0), "1970-01-01 00:00 UTC");
        for ms in [-86_400_000_i64 * 1000, 951_782_400_000, 4_102_444_800_000] {
            let text = format_time_utc(ms);
            assert_eq!(
                iso_time_ms(&text[..10]).unwrap(),
                ms - ms.rem_euclid(86_400_000),
                "{text}"
            );
        }
    }
}
