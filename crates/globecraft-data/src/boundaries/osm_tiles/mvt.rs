//! A minimal reader for Mapbox Vector Tiles (protobuf), enough for the boundary and
//! place layers of OpenMapTiles: layer names, feature tags and point/line geometry.
//! Specification: <https://github.com/mapbox/vector-tile-spec/tree/master/2.1>.

/// Tile coordinates of a vector tile run from 0 to the layer's extent (default 4096).
const DEFAULT_EXTENT: u32 = 4096;

/// A tag value of a feature.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    Text(String),
    Number(f64),
    Bool(bool),
}

impl Value {
    pub(super) fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Bool(b) => Some(f64::from(u8::from(*b))),
            Self::Text(_) => None,
        }
    }
    pub(super) fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// Geometry kinds of the specification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum GeomType {
    Unknown,
    Point,
    Line,
    Polygon,
}

#[derive(Debug)]
pub(super) struct Feature {
    /// Pairs of indices into the layer's `keys` and `values`.
    tags: Vec<(u32, u32)>,
    pub(super) kind: GeomType,
    /// Points or line parts, in tile units.
    pub(super) parts: Vec<Vec<(f64, f64)>>,
}

#[derive(Debug)]
pub(super) struct Layer {
    pub(super) name: String,
    pub(super) extent: u32,
    keys: Vec<String>,
    values: Vec<Value>,
    pub(super) features: Vec<Feature>,
}

impl Layer {
    /// The value of a feature's tag `key`.
    pub(super) fn tag<'a>(&'a self, feature: &Feature, key: &str) -> Option<&'a Value> {
        feature.tags.iter().find_map(|&(k, v)| {
            (self.keys.get(k as usize).map(String::as_str) == Some(key))
                .then(|| self.values.get(v as usize))
                .flatten()
        })
    }
}

/// Protobuf wire reader (also reads the varints of the compact cache format).
pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub(super) fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
    pub(super) fn varint(&mut self) -> Option<u64> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.bytes.get(self.pos)?;
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    pub(super) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(len)?;
        let slice = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }
    pub(super) fn bytes(&mut self) -> Option<&'a [u8]> {
        let len = usize::try_from(self.varint()?).ok()?;
        self.take(len)
    }
    /// The next field number and wire type.
    fn key(&mut self) -> Option<(u32, u8)> {
        let key = self.varint()?;
        Some((u32::try_from(key >> 3).ok()?, (key & 7) as u8))
    }
    /// Skips a field's value of the given wire type.
    fn skip(&mut self, wire: u8) -> Option<()> {
        match wire {
            0 => self.varint().map(|_| ()),
            1 => self.take(8).map(|_| ()),
            2 => self.bytes().map(|_| ()),
            5 => self.take(4).map(|_| ()),
            _ => None,
        }
    }
    /// A repeated uint32 field, packed or not.
    fn uints(&mut self, wire: u8, out: &mut Vec<u32>) -> Option<()> {
        if wire == 2 {
            let mut packed = Reader::new(self.bytes()?);
            while !packed.done() {
                out.push(packed.varint()? as u32);
            }
        } else {
            out.push(self.varint()? as u32);
        }
        Some(())
    }
}

fn zigzag(value: u32) -> i64 {
    i64::from(value >> 1) ^ -i64::from(value & 1)
}

/// The layers of a tile whose names `wanted` accepts; `None` if the tile is malformed.
pub(super) fn decode(bytes: &[u8], wanted: impl Fn(&str) -> bool) -> Option<Vec<Layer>> {
    let mut reader = Reader::new(bytes);
    let mut layers = Vec::new();
    while !reader.done() {
        let (field, wire) = reader.key()?;
        if field == 3 && wire == 2 {
            if let Some(layer) = decode_layer(reader.bytes()?, &wanted)? {
                layers.push(layer);
            }
        } else {
            reader.skip(wire)?;
        }
    }
    Some(layers)
}

/// `Some(None)`: a well-formed layer that is not wanted.
fn decode_layer(bytes: &[u8], wanted: &impl Fn(&str) -> bool) -> Option<Option<Layer>> {
    // The name may follow the features, so features are kept raw until it is known.
    let mut reader = Reader::new(bytes);
    let (mut name, mut extent) = (String::new(), DEFAULT_EXTENT);
    let (mut keys, mut values, mut raw_features) = (Vec::new(), Vec::new(), Vec::new());
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match (field, wire) {
            (1, 2) => name = String::from_utf8_lossy(reader.bytes()?).into_owned(),
            (2, 2) => raw_features.push(reader.bytes()?),
            (3, 2) => keys.push(String::from_utf8_lossy(reader.bytes()?).into_owned()),
            (4, 2) => values.push(decode_value(reader.bytes()?)?),
            (5, 0) => extent = u32::try_from(reader.varint()?).ok()?,
            _ => reader.skip(wire)?,
        }
    }
    if !wanted(&name) {
        return Some(None);
    }
    let features = raw_features
        .into_iter()
        .map(decode_feature)
        .collect::<Option<Vec<_>>>()?;
    Some(Some(Layer {
        name,
        extent,
        keys,
        values,
        features,
    }))
}

fn decode_value(bytes: &[u8]) -> Option<Value> {
    let mut reader = Reader::new(bytes);
    let mut value = Value::Bool(false);
    while !reader.done() {
        let (field, wire) = reader.key()?;
        value = match (field, wire) {
            (1, 2) => Value::Text(String::from_utf8_lossy(reader.bytes()?).into_owned()),
            (2, 5) => Value::Number(f64::from(f32::from_le_bytes(
                reader.take(4)?.try_into().ok()?,
            ))),
            (3, 1) => Value::Number(f64::from_le_bytes(reader.take(8)?.try_into().ok()?)),
            (4, 0) => Value::Number(reader.varint()? as i64 as f64),
            (5, 0) => Value::Number(reader.varint()? as f64),
            (6, 0) => {
                let raw = reader.varint()?;
                Value::Number(((raw >> 1) as i64 ^ -((raw & 1) as i64)) as f64)
            }
            (7, 0) => Value::Bool(reader.varint()? != 0),
            _ => {
                reader.skip(wire)?;
                continue;
            }
        };
    }
    Some(value)
}

fn decode_feature(bytes: &[u8]) -> Option<Feature> {
    let mut reader = Reader::new(bytes);
    let (mut tags, mut geometry, mut kind) = (Vec::new(), Vec::new(), GeomType::Unknown);
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match field {
            2 => reader.uints(wire, &mut tags)?,
            3 if wire == 0 => {
                kind = match reader.varint()? {
                    1 => GeomType::Point,
                    2 => GeomType::Line,
                    3 => GeomType::Polygon,
                    _ => GeomType::Unknown,
                }
            }
            4 => reader.uints(wire, &mut geometry)?,
            _ => reader.skip(wire)?,
        }
    }
    Some(Feature {
        tags: tags
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[k, v]| (k, v))
            .collect(),
        kind,
        parts: decode_geometry(&geometry, kind)?,
    })
}

/// Command integers to parts: points (each its own part) or line/ring parts.
fn decode_geometry(commands: &[u32], kind: GeomType) -> Option<Vec<Vec<(f64, f64)>>> {
    let (mut x, mut y) = (0_i64, 0_i64);
    let mut parts: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut i = 0;
    while i < commands.len() {
        let (command, count) = (commands[i] & 7, (commands[i] >> 3) as usize);
        i += 1;
        match command {
            // MoveTo and LineTo carry `count` coordinate pairs.
            1 | 2 => {
                let params = commands.get(i..i.checked_add(count * 2)?)?;
                i += count * 2;
                for pair in params.as_chunks::<2>().0 {
                    x += zigzag(pair[0]);
                    y += zigzag(pair[1]);
                    let point = (x as f64, y as f64);
                    if command == 1 || parts.is_empty() {
                        parts.push(vec![point]);
                    } else if let Some(part) = parts.last_mut() {
                        part.push(point);
                    }
                }
            }
            // ClosePath: rings repeat their first point.
            7 => {
                if kind == GeomType::Polygon
                    && let Some(part) = parts.last_mut()
                    && let Some(&first) = part.first()
                {
                    part.push(first);
                }
            }
            _ => return None,
        }
    }
    Some(parts)
}

#[cfg(test)]
pub(super) mod encode {
    //! A tiny encoder for tests, so the decoder is checked against the specification's
    //! wire format rather than against itself.
    fn varint(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }
    fn field(number: u32, wire: u8, out: &mut Vec<u8>) {
        varint(u64::from(number) << 3 | u64::from(wire), out);
    }
    fn bytes(number: u32, data: &[u8], out: &mut Vec<u8>) {
        field(number, 2, out);
        varint(data.len() as u64, out);
        out.extend_from_slice(data);
    }
    fn zigzag(v: i32) -> u32 {
        ((v << 1) ^ (v >> 31)) as u32
    }
    pub(crate) enum TestValue<'a> {
        Text(&'a str),
        Int(i64),
    }
    /// A feature: tags as (key, value), geometry type, parts in tile units.
    pub(crate) struct TestFeature<'a> {
        pub(crate) tags: Vec<(&'a str, TestValue<'a>)>,
        pub(crate) kind: u32,
        pub(crate) parts: Vec<Vec<(i32, i32)>>,
    }
    pub(crate) fn layer(name: &str, extent: u32, features: &[TestFeature<'_>]) -> Vec<u8> {
        let mut keys: Vec<&str> = Vec::new();
        let mut values: Vec<Vec<u8>> = Vec::new();
        let mut out = Vec::new();
        field(15, 0, &mut out);
        varint(2, &mut out);
        bytes(1, name.as_bytes(), &mut out);
        for feature in features {
            let mut tags = Vec::new();
            for (key, value) in &feature.tags {
                let k = keys.iter().position(|k| k == key).unwrap_or_else(|| {
                    keys.push(key);
                    keys.len() - 1
                });
                let mut encoded = Vec::new();
                match value {
                    TestValue::Text(text) => bytes(1, text.as_bytes(), &mut encoded),
                    TestValue::Int(n) => {
                        field(4, 0, &mut encoded);
                        varint(*n as u64, &mut encoded);
                    }
                }
                values.push(encoded);
                varint(k as u64, &mut tags);
                varint(values.len() as u64 - 1, &mut tags);
            }
            let mut geometry = Vec::new();
            let (mut x, mut y) = (0, 0);
            for part in &feature.parts {
                varint(u64::from(1_u32 | 1 << 3), &mut geometry);
                varint(u64::from(zigzag(part[0].0 - x)), &mut geometry);
                varint(u64::from(zigzag(part[0].1 - y)), &mut geometry);
                (x, y) = part[0];
                if part.len() > 1 {
                    varint(u64::from(2 | (part.len() as u32 - 1) << 3), &mut geometry);
                    for &(px, py) in &part[1..] {
                        varint(u64::from(zigzag(px - x)), &mut geometry);
                        varint(u64::from(zigzag(py - y)), &mut geometry);
                        (x, y) = (px, py);
                    }
                }
            }
            let mut encoded = Vec::new();
            bytes(2, &tags, &mut encoded);
            field(3, 0, &mut encoded);
            varint(u64::from(feature.kind), &mut encoded);
            bytes(4, &geometry, &mut encoded);
            bytes(2, &encoded, &mut out);
        }
        for key in keys {
            bytes(3, key.as_bytes(), &mut out);
        }
        for value in values {
            bytes(4, &value, &mut out);
        }
        field(5, 0, &mut out);
        varint(u64::from(extent), &mut out);
        out
    }
    pub(crate) fn tile(layers: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for layer in layers {
            bytes(3, layer, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::encode::{TestFeature, TestValue, layer, tile};
    use super::*;

    #[test]
    fn decodes_lines_points_and_tags_of_wanted_layers_only() {
        let bytes = tile(&[
            layer(
                "boundary",
                4096,
                &[TestFeature {
                    tags: vec![
                        ("admin_level", TestValue::Int(2)),
                        ("maritime", TestValue::Int(0)),
                    ],
                    kind: 2,
                    parts: vec![
                        vec![(10, 20), (30, 25), (4000, 4096)],
                        vec![(-5, 7), (8, 9)],
                    ],
                }],
            ),
            layer(
                "place",
                4096,
                &[TestFeature {
                    tags: vec![
                        ("class", TestValue::Text("island")),
                        ("name", TestValue::Text("Fais")),
                    ],
                    kind: 1,
                    parts: vec![vec![(100, 200)]],
                }],
            ),
            layer("water", 4096, &[]),
        ]);
        let layers = decode(&bytes, |name| name != "water").unwrap();
        assert_eq!(layers.len(), 2);
        let boundary = &layers[0];
        assert_eq!(
            (boundary.name.as_str(), boundary.extent),
            ("boundary", 4096)
        );
        let feature = &boundary.features[0];
        assert_eq!(feature.kind, GeomType::Line);
        assert_eq!(
            feature.parts,
            vec![
                vec![(10.0, 20.0), (30.0, 25.0), (4000.0, 4096.0)],
                vec![(-5.0, 7.0), (8.0, 9.0)]
            ]
        );
        assert_eq!(
            boundary.tag(feature, "admin_level"),
            Some(&Value::Number(2.0))
        );
        assert_eq!(boundary.tag(feature, "disputed"), None);
        let place = &layers[1];
        assert_eq!(place.features[0].kind, GeomType::Point);
        assert_eq!(place.features[0].parts, vec![vec![(100.0, 200.0)]]);
        assert_eq!(
            place
                .tag(&place.features[0], "name")
                .and_then(Value::as_text),
            Some("Fais")
        );
    }

    #[test]
    fn malformed_tiles_are_rejected_not_panicking() {
        let good = tile(&[layer("boundary", 4096, &[])]);
        for cut in 0..good.len() {
            let _ = decode(&good[..cut], |_| true);
        }
        assert!(decode(&[0xff; 12], |_| true).is_none());
        assert_eq!(decode(&[], |_| true).map(|l| l.len()), Some(0));
    }
}

/// Decodes a real tile: `GLOBECRAFT_MVT_FIXTURE=<file.pbf> cargo test -p globecraft-data
/// real_tile_decodes -- --ignored --nocapture` (any schema; prints its layers).
#[cfg(test)]
#[test]
#[ignore = "needs a tile file"]
fn real_tile_decodes() {
    let path = std::env::var("GLOBECRAFT_MVT_FIXTURE").expect("GLOBECRAFT_MVT_FIXTURE");
    let bytes = std::fs::read(path).unwrap();
    let layers = decode(&bytes, |_| true).expect("well-formed tile");
    assert!(!layers.is_empty());
    for layer in &layers {
        let points: usize = layer
            .features
            .iter()
            .flat_map(|f| &f.parts)
            .map(Vec::len)
            .sum();
        let outside = layer
            .features
            .iter()
            .flat_map(|f| f.parts.iter().flatten())
            .filter(|&&(x, y)| {
                let (lo, hi) = (-(layer.extent as f64), 2.0 * layer.extent as f64);
                !(lo..hi).contains(&x) || !(lo..hi).contains(&y)
            })
            .count();
        println!(
            "{:<20} extent {} features {:>5} points {:>6} far outside {}",
            layer.name,
            layer.extent,
            layer.features.len(),
            points,
            outside
        );
        assert_eq!(outside, 0, "{}: coordinates far off the tile", layer.name);
    }
}
