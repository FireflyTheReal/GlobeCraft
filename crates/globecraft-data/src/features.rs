//! Stream a feature collection, retaining projected data rather than the JSON tree.
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;

struct Collection<'a, T, F> {
    out: &'a mut T,
    consume: &'a F,
}
impl<'de, T, F: Fn(&serde_json::Value, &mut T)> DeserializeSeed<'de> for Collection<'_, T, F> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de, T, F: Fn(&serde_json::Value, &mut T)> Visitor<'de> for Collection<'_, T, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a GeoJSON feature collection")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let mut found = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == "features" {
                map.next_value_seed(Features {
                    out: self.out,
                    consume: self.consume,
                })?;
                found = true;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        if !found {
            return Err(serde::de::Error::custom(
                "GeoJSON document is missing 'features'",
            ));
        }
        Ok(())
    }
}
struct Features<'a, T, F> {
    out: &'a mut T,
    consume: &'a F,
}
impl<'de, T, F: Fn(&serde_json::Value, &mut T)> DeserializeSeed<'de> for Features<'_, T, F> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, T, F: Fn(&serde_json::Value, &mut T)> Visitor<'de> for Features<'_, T, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a feature array")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
        while let Some(feature) = seq.next_element::<serde_json::Value>()? {
            (self.consume)(&feature, self.out);
        }
        Ok(())
    }
}
pub(crate) fn project<T: Default, F: Fn(&serde_json::Value, &mut T)>(
    reader: impl std::io::Read,
    consume: F,
) -> Result<T, crate::Error> {
    let mut out = T::default();
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    Collection {
        out: &mut out,
        consume: &consume,
    }
    .deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaming_projection_skips_metadata_and_preserves_order() {
        let json=br#"{"type":"FeatureCollection","metadata":{"unused":[1,2,3]},"features":[{"id":1},{"id":2}]}"#;
        let ids: Vec<u64> = project(json.as_slice(), |value, out: &mut Vec<u64>| {
            out.push(value["id"].as_u64().unwrap())
        })
        .unwrap();
        assert_eq!(ids, vec![1, 2]);
        assert!(project::<Vec<u64>, _>(b"{}".as_slice(), |_, _| {}).is_err());
        assert!(project::<Vec<u64>, _>(b"{\"features\":[{}]".as_slice(), |_, _| {}).is_err());
    }
}
