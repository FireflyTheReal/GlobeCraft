//! Features of a user layer: points with their names, and lines (tracks, routes and
//! the outlines of areas), read from GeoJSON, GPX or CSV with coordinate columns, from
//! a file or a URL. A URL that is a live feed is read again at its interval.
use super::FeatureFormat;
use super::properties::{PointProperties, PropertyReader, Scalar};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The largest file read.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// More vertices than this are left out (the layer says so).
const MAX_VERTICES: usize = 2_000_000;
/// A failed load of a URL is tried again after this long.
const RETRY: Duration = Duration::from_secs(60);
/// The byte-order mark some programs write at the start of a UTF-8 file.
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

#[derive(Clone, Debug, PartialEq)]
pub struct NamedPoint {
    pub lat: f64,
    pub lon: f64,
    pub name: String,
    /// The value its marker is sized and ranked by (`FeatureSet::value_field`).
    pub value: Option<f64>,
    /// When it happened, milliseconds since 1970, when its properties say.
    pub time_ms: Option<i64>,
    /// A few other properties, one `key: value` per line, for its hover card.
    pub details: String,
}

/// What a layer draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureSet {
    pub points: Vec<NamedPoint>,
    /// Lines and the rings of areas, in latitude/longitude.
    pub lines: Vec<Vec<(f64, f64)>>,
    /// South, west, north, east of everything, when there is anything.
    pub bounds: Option<[f64; 4]>,
    /// Whether vertices were left out over `MAX_VERTICES`.
    pub truncated: bool,
    /// The property the points' values come from, and the smallest and largest value.
    pub value_field: Option<String>,
    pub value_range: Option<(f64, f64)>,
    /// Points and line vertices kept so far, counted as they are added: summing the
    /// lines for every new feature made reading a file of many lines quadratic.
    vertices: usize,
}

impl FeatureSet {
    fn room(&self) -> bool {
        self.vertices < MAX_VERTICES
    }
    fn point(&mut self, lat: f64, lon: f64, name: String, properties: PointProperties) {
        if !valid(lat, lon) {
            return;
        }
        if !self.room() {
            self.truncated = true;
            return;
        }
        let PointProperties {
            value,
            time_ms,
            details,
        } = properties;
        self.points.push(NamedPoint {
            lat,
            lon,
            name,
            value,
            time_ms,
            details,
        });
        self.vertices += 1;
    }
    fn line(&mut self, line: Vec<(f64, f64)>) {
        let line: Vec<(f64, f64)> = line
            .into_iter()
            .filter(|&(lat, lon)| valid(lat, lon))
            .collect();
        if line.len() < 2 {
            return;
        }
        if !self.room() {
            self.truncated = true;
            return;
        }
        self.vertices += line.len();
        self.lines.push(line);
    }
    fn finish(mut self) -> Self {
        let coordinates = self
            .points
            .iter()
            .map(|p| (p.lat, p.lon))
            .chain(self.lines.iter().flatten().copied());
        let mut bounds: Option<[f64; 4]> = None;
        for (lat, lon) in coordinates {
            let b = bounds.get_or_insert([lat, lon, lat, lon]);
            *b = [b[0].min(lat), b[1].min(lon), b[2].max(lat), b[3].max(lon)];
        }
        self.bounds = bounds;
        self.value_range = self
            .points
            .iter()
            .filter_map(|p| p.value)
            .fold(None, |range, v| {
                Some(range.map_or((v, v), |(low, high): (f64, f64)| (low.min(v), high.max(v))))
            });
        if self.value_range.is_none() {
            self.value_field = None;
        }
        self
    }
}

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

/// Reads features in `format` from `bytes`; point values come from the property
/// `value_field`, or from the first of the usual ones (`mag`, `population`…) found.
pub fn parse_features(
    format: FeatureFormat,
    bytes: &[u8],
    value_field: Option<&str>,
) -> Result<FeatureSet, crate::Error> {
    // Spreadsheets and some servers start a file with a byte-order mark: it is not
    // part of the first column's name, and JSON does not allow it.
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let mut reader = PropertyReader::new(value_field);
    let mut set = match format {
        FeatureFormat::GeoJson => geojson(bytes, &mut reader)?,
        FeatureFormat::Gpx => gpx(bytes)?,
        FeatureFormat::Csv => csv(bytes, &mut reader)?,
    };
    set.value_field = reader.value_field().map(str::to_owned);
    let set = set.finish();
    if set.points.is_empty() && set.lines.is_empty() {
        return Err("no points or lines with coordinates were found".into());
    }
    Ok(set)
}

// ---- GeoJSON ----

fn geojson(bytes: &[u8], reader: &mut PropertyReader) -> Result<FeatureSet, crate::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let mut set = FeatureSet::default();
    geojson_object(&value, &Named::default(), &mut set, reader);
    Ok(set)
}

/// The name and properties a feature gives its geometry.
#[derive(Clone, Debug, Default)]
struct Named {
    name: String,
    properties: PointProperties,
}

/// A feature's scalar properties, for the property reader.
fn scalars(
    properties: Option<&serde_json::Value>,
) -> impl Iterator<Item = (&str, Scalar<'_>)> + Clone {
    properties
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(text) => Some((key.as_str(), Scalar::Text(text))),
            serde_json::Value::Number(n) => Some((key.as_str(), Scalar::Number(n.as_f64()?))),
            serde_json::Value::Bool(b) => Some((
                key.as_str(),
                Scalar::Text(if *b { "true" } else { "false" }),
            )),
            _ => None,
        })
}

/// The name a feature's properties give it.
fn feature_name(properties: Option<&serde_json::Value>) -> String {
    let Some(properties) = properties else {
        return String::new();
    };
    for key in ["name", "Name", "NAME", "title", "Title", "place", "label"] {
        if let Some(text) = properties.get(key).and_then(serde_json::Value::as_str) {
            return text.to_owned();
        }
    }
    String::new()
}

fn geojson_object(
    value: &serde_json::Value,
    named: &Named,
    set: &mut FeatureSet,
    reader: &mut PropertyReader,
) {
    let kind = value.get("type").and_then(serde_json::Value::as_str);
    match kind {
        Some("FeatureCollection") => {
            for feature in value
                .get("features")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                geojson_object(feature, &Named::default(), set, reader);
            }
        }
        Some("Feature") => {
            let properties = value.get("properties");
            let named = Named {
                name: feature_name(properties),
                properties: reader.read(scalars(properties)),
            };
            if let Some(geometry) = value.get("geometry") {
                geojson_object(geometry, &named, set, reader);
            }
        }
        Some("GeometryCollection") => {
            for geometry in value
                .get("geometries")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                geojson_object(geometry, named, set, reader);
            }
        }
        Some(kind) => {
            let Some(coordinates) = value.get("coordinates") else {
                return;
            };
            match kind {
                "Point" => {
                    if let Some((lat, lon)) = position(coordinates) {
                        set.point(lat, lon, named.name.clone(), named.properties.clone());
                    }
                }
                "MultiPoint" => {
                    for (lat, lon) in positions(coordinates) {
                        set.point(lat, lon, named.name.clone(), named.properties.clone());
                    }
                }
                "LineString" => set.line(positions(coordinates)),
                "MultiLineString" | "Polygon" => {
                    for part in coordinates.as_array().into_iter().flatten() {
                        set.line(positions(part));
                    }
                }
                "MultiPolygon" => {
                    for polygon in coordinates.as_array().into_iter().flatten() {
                        for ring in polygon.as_array().into_iter().flatten() {
                            set.line(positions(ring));
                        }
                    }
                }
                _ => {}
            }
        }
        None => {}
    }
}

/// `[lon, lat, ...]` as latitude, longitude.
fn position(value: &serde_json::Value) -> Option<(f64, f64)> {
    let array = value.as_array()?;
    Some((array.get(1)?.as_f64()?, array.first()?.as_f64()?))
}

fn positions(value: &serde_json::Value) -> Vec<(f64, f64)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(position)
        .collect()
}

// ---- GPX ----

fn gpx(bytes: &[u8]) -> Result<FeatureSet, crate::Error> {
    use quick_xml::events::Event;
    // Text is not trimmed by the reader: a name with `&amp;` or CDATA arrives in
    // pieces, and trimming each would join its words.
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut set = FeatureSet::default();
    let mut buffer = Vec::new();
    // The point being read (waypoint, or a track or route point), its name, and the
    // track segment or route being collected.
    let mut point: Option<(f64, f64, bool)> = None;
    let mut name = String::new();
    let mut in_name = false;
    let mut line: Vec<(f64, f64)> = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(element) | Event::Empty(element)
                if matches!(element.local_name().as_ref(), "trkpt" | "rtept") =>
            {
                if let Some(at) = coordinates(&element) {
                    line.push(at);
                }
            }
            // `<wpt .../>`: no name element follows.
            Event::Empty(element) if element.local_name().as_ref() == "wpt" => {
                if let Some((lat, lon)) = coordinates(&element) {
                    set.point(lat, lon, String::new(), PointProperties::default());
                }
            }
            Event::Start(element) if element.local_name().as_ref() == "wpt" => {
                point = coordinates(&element).map(|(lat, lon)| (lat, lon, true));
                name.clear();
            }
            Event::Start(element) if element.local_name().as_ref() == "name" => {
                in_name = point.is_some();
            }
            Event::Text(text) if in_name => {
                name.push_str(&text.xml10_content());
            }
            Event::CData(text) if in_name => {
                name.push_str(&text.xml10_content());
            }
            // `&amp;`, `&#233;`: the reader hands references over on their own.
            Event::GeneralRef(reference) if in_name => {
                if let Some(text) = super::capabilities::reference_text(&reference) {
                    name.push_str(&text);
                }
            }
            Event::End(element) => match element.local_name().as_ref() {
                "name" => in_name = false,
                "wpt" => {
                    if let Some((lat, lon, _)) = point.take() {
                        let label = name.trim().to_owned();
                        name.clear();
                        set.point(lat, lon, label, PointProperties::default());
                    }
                }
                "trkseg" | "rte" => set.line(std::mem::take(&mut line)),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    // A self-closing waypoint without a name element.
    if let Some((lat, lon, _)) = point.take() {
        set.point(lat, lon, name.trim().to_owned(), PointProperties::default());
    }
    set.line(line);
    Ok(set)
}

/// A GPX point's `lat` and `lon` attributes.
fn coordinates(element: &quick_xml::events::BytesStart) -> Option<(f64, f64)> {
    let mut lat = None;
    let mut lon = None;
    for attribute in element.attributes().flatten() {
        let value = attribute.value.trim().parse::<f64>().ok();
        match attribute.key.local_name().as_ref() {
            "lat" => lat = value,
            "lon" => lon = value,
            _ => {}
        }
    }
    Some((lat?, lon?))
}

// ---- CSV ----

fn csv(bytes: &[u8], reader: &mut PropertyReader) -> Result<FeatureSet, crate::Error> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("the file is empty")?;
    let delimiter = [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|d| header.matches(*d).count())
        .unwrap_or(',');
    // The column names as written (for the hover card) and in lower case (to find
    // the coordinates).
    let headers: Vec<String> = split(header, delimiter);
    let columns: Vec<String> = headers.iter().map(|c| c.to_ascii_lowercase()).collect();
    let find = |names: &[&str]| columns.iter().position(|c| names.contains(&c.as_str()));
    let lat = find(&["lat", "latitude", "y", "lat_dd", "decimallatitude"])
        .ok_or("no latitude column (lat, latitude or y)")?;
    let lon = find(&[
        "lon",
        "lng",
        "long",
        "longitude",
        "x",
        "lon_dd",
        "decimallongitude",
    ])
    .ok_or("no longitude column (lon, lng, longitude or x)")?;
    let name = find(&["name", "title", "label", "place", "city"]);
    let mut set = FeatureSet::default();
    for line in lines {
        let fields = split(line, delimiter);
        let number = |i: usize| {
            fields
                .get(i)
                .and_then(|f| f.replace(',', ".").parse::<f64>().ok())
        };
        if let (Some(la), Some(lo)) = (number(lat), number(lon)) {
            let label = name
                .and_then(|i| fields.get(i))
                .cloned()
                .unwrap_or_default();
            let others = headers
                .iter()
                .zip(&fields)
                .enumerate()
                .filter(|(i, _)| *i != lat && *i != lon && Some(*i) != name)
                .map(|(_, (key, cell))| (key.as_str(), Scalar::Text(cell)));
            set.point(la, lo, label, reader.read(others));
        }
    }
    Ok(set)
}

/// A CSV line's fields, trimmed, honouring double quotes: a quoted field may hold
/// the delimiter, and `""` inside it is one quote.
fn split(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => {
                fields.push(field.trim().to_owned());
                field.clear();
            }
            c => field.push(c),
        }
    }
    fields.push(field.trim().to_owned());
    fields
}

// ---- Loading ----

#[derive(Default)]
struct Shared {
    features: Option<Arc<FeatureSet>>,
    error: Option<String>,
    loading: bool,
    loaded_at: Option<Instant>,
    failed_at: Option<Instant>,
    /// Bumped whenever new features arrive.
    revision: u64,
}

/// One feature layer: loads its file or URL, again at the refresh interval for a URL.
pub struct VectorLayer {
    location: String,
    format: FeatureFormat,
    /// The property point values come from (`None`: the usual ones).
    value_field: Option<String>,
    refresh: Option<Duration>,
    shared: Arc<Mutex<Shared>>,
}

impl VectorLayer {
    pub fn new(
        location: &str,
        format: FeatureFormat,
        refresh_minutes: u32,
        value_field: Option<&str>,
    ) -> Self {
        Self {
            location: location.trim().to_owned(),
            format,
            value_field: value_field.map(str::to_owned),
            refresh: (super::is_url(location) && refresh_minutes > 0)
                .then(|| Duration::from_secs(u64::from(refresh_minutes) * 60)),
            shared: Arc::default(),
        }
    }

    /// Starts a load when one is due; call every frame the layer is shown.
    pub fn update(
        &self,
        runtime: &crate::runtime::ServiceRuntime,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        let now = Instant::now();
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let due = !shared.loading
            && match (shared.loaded_at, shared.failed_at) {
                (_, Some(failed)) if now.duration_since(failed) < RETRY => false,
                (None, _) => true,
                (Some(loaded), _) => self
                    .refresh
                    .is_some_and(|every| now.duration_since(loaded) >= every),
            };
        if !due {
            return;
        }
        shared.loading = true;
        let (location, format, state) = (self.location.clone(), self.format, self.shared.clone());
        let value_field = self.value_field.clone();
        runtime.spawn(async move {
            let result = load(location, format, value_field).await;
            if let Ok(mut shared) = state.lock() {
                shared.loading = false;
                match result {
                    Ok(features) => {
                        shared.features = Some(Arc::new(features));
                        shared.loaded_at = Some(Instant::now());
                        shared.failed_at = None;
                        shared.error = None;
                        shared.revision += 1;
                    }
                    Err(error) => {
                        shared.failed_at = Some(Instant::now());
                        shared.error = Some(error.to_string());
                    }
                }
            }
            repaint();
        });
    }

    /// The features and their revision, once loaded.
    pub fn features(&self) -> Option<(Arc<FeatureSet>, u64)> {
        let shared = self.shared.lock().ok()?;
        Some((shared.features.clone()?, shared.revision))
    }

    pub fn error(&self) -> Option<String> {
        self.shared.lock().ok()?.error.clone()
    }

    pub fn loading(&self) -> bool {
        self.shared.lock().is_ok_and(|shared| shared.loading)
    }
}

/// Reads and parses a file or URL.
pub async fn load(
    location: String,
    format: FeatureFormat,
    value_field: Option<String>,
) -> Result<FeatureSet, crate::Error> {
    // One byte over the limit is read, to tell a file that is too large from one
    // that just fits: a file cut off at the limit used to fail as broken JSON.
    let bytes = if super::is_url(&location) {
        let client = crate::http::client(crate::http::Policy::UserData)?;
        let address = crate::keys::fill(&location)?;
        crate::http::body(client.get(&address), MAX_BYTES + 1)
            .await
            .map_err(|error| crate::keys::without_address(error, &location))?
    } else {
        crate::cpu::run(crate::cpu::Service::Files, move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(&location)?
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .await?
    };
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!(
            "the file is larger than {} MiB, the most a layer reads",
            MAX_BYTES / (1024 * 1024)
        )
        .into());
    }
    crate::cpu::run(crate::cpu::Service::Features, move || {
        parse_features(format, &bytes, value_field.as_deref())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geojson_gives_named_points_lines_and_area_outlines() {
        let json = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"title":"M 4.6 - Japan"},
             "geometry":{"type":"Point","coordinates":[142.3,38.1,10.0]}},
            {"type":"Feature","properties":null,
             "geometry":{"type":"LineString","coordinates":[[0,0],[1,1]]}},
            {"type":"Feature","properties":{"name":"Area"},
             "geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[0,1],[1,1],[0,0]]]]}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[500,0]}}
        ]}"#;
        let set = parse_features(FeatureFormat::GeoJson, json, None).unwrap();
        assert_eq!(
            set.points,
            [NamedPoint {
                lat: 38.1,
                lon: 142.3,
                name: "M 4.6 - Japan".into(),
                value: None,
                time_ms: None,
                details: String::new(),
            }]
        );
        assert_eq!(set.lines.len(), 2);
        assert_eq!(set.bounds, Some([0.0, 0.0, 38.1, 142.3]));
    }

    #[test]
    fn gpx_gives_waypoints_tracks_and_routes() {
        let gpx = br#"<?xml version="1.0"?>
            <gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
              <wpt lat="48.1" lon="11.5"><name>Hut</name></wpt>
              <wpt lat="48.2" lon="11.6"/>
              <trk><name>Walk</name><trkseg>
                <trkpt lat="48.0" lon="11.0"/><trkpt lat="48.01" lon="11.01"><ele>500</ele></trkpt>
              </trkseg></trk>
              <rte><rtept lat="1" lon="2"/><rtept lat="3" lon="4"/></rte>
            </gpx>"#;
        let set = parse_features(FeatureFormat::Gpx, gpx, None).unwrap();
        let names: Vec<&str> = set.points.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Hut", ""]);
        assert_eq!(
            set.lines,
            vec![
                vec![(48.0, 11.0), (48.01, 11.01)],
                vec![(1.0, 2.0), (3.0, 4.0)]
            ]
        );
    }

    #[test]
    fn csv_finds_its_coordinate_columns_and_delimiter() {
        let csv = "Name;Latitude;Longitude\n\"Lisbon, PT\";38,72;-9,14\nBad;x;1\n";
        let set = parse_features(FeatureFormat::Csv, csv.as_bytes(), None).unwrap();
        assert_eq!(
            set.points,
            [NamedPoint {
                lat: 38.72,
                lon: -9.14,
                name: "Lisbon, PT".into(),
                value: None,
                time_ms: None,
                details: String::new(),
            }]
        );
        let error = parse_features(FeatureFormat::Csv, b"a,b\n1,2\n", None).unwrap_err();
        assert!(error.to_string().contains("latitude"), "{error}");
    }

    #[test]
    fn files_with_a_byte_order_mark_read_like_any_other() {
        // "CSV UTF-8" from a spreadsheet: the mark is not part of the first column.
        let csv = "\u{feff}lat,lon,name\n38.72,-9.14,Lisbon\n";
        let set = parse_features(FeatureFormat::Csv, csv.as_bytes(), None).unwrap();
        assert_eq!(set.points[0].name, "Lisbon");
        let json = "\u{feff}{\"type\":\"Point\",\"coordinates\":[1,2]}";
        let set = parse_features(FeatureFormat::GeoJson, json.as_bytes(), None).unwrap();
        assert_eq!((set.points[0].lat, set.points[0].lon), (2.0, 1.0));
        let gpx = "\u{feff}<gpx><wpt lat=\"1\" lon=\"2\"/></gpx>";
        assert!(parse_features(FeatureFormat::Gpx, gpx.as_bytes(), None).is_ok());
    }

    #[test]
    fn gpx_names_keep_entities_character_data_and_inner_spaces() {
        let gpx = br#"<gpx>
              <wpt lat="48.1" lon="11.5"><name> Tom &amp; Jerry&apos;s caf&#233; </name></wpt>
              <wpt lat="48.2" lon="11.6"><name><![CDATA[Hut <2,000 m>]]></name></wpt>
            </gpx>"#;
        let set = parse_features(FeatureFormat::Gpx, gpx, None).unwrap();
        let names: Vec<&str> = set.points.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Tom & Jerry's caf\u{e9}", "Hut <2,000 m>"]);
    }

    #[test]
    fn csv_quotes_hold_delimiters_and_doubled_quotes() {
        let csv = "name,lat,lon,note\n\"Porto, \"\"the\"\" city\",41.15,-8.61,\"a, b\"\n";
        let set = parse_features(FeatureFormat::Csv, csv.as_bytes(), None).unwrap();
        assert_eq!(set.points[0].name, "Porto, \"the\" city");
        assert_eq!(set.points[0].details, "note: a, b");
        assert_eq!(split(" a ;\"b;c\"; ", ';'), ["a", "b;c", ""]);
    }

    #[test]
    fn many_lines_are_counted_once_and_stop_at_the_vertex_limit() {
        // 150,000 short lines: counting every earlier line for each new one took
        // over ten thousand million steps.
        let mut set = FeatureSet::default();
        let started = Instant::now();
        for i in 0..150_000 {
            let lat = f64::from(i % 80);
            set.line(vec![(lat, 0.0), (lat, 1.0), (lat, 2.0)]);
        }
        assert_eq!(set.vertices, 450_000);
        assert!(started.elapsed() < Duration::from_secs(5));
        set.vertices = MAX_VERTICES - 1;
        set.point(0.0, 0.0, String::new(), PointProperties::default());
        set.point(0.0, 0.0, String::new(), PointProperties::default());
        assert!(set.truncated && set.vertices == MAX_VERTICES);
    }

    #[test]
    fn points_carry_their_value_time_and_details() {
        let json = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"title":"M 6.1 - Chile","mag":6.1,
              "time":1793700000000,"tsunami":1,"url":"https://example.org"},
             "geometry":{"type":"Point","coordinates":[-71.0,-33.0]}},
            {"type":"Feature","properties":{"title":"M 2.5 - Alaska","mag":2.5},
             "geometry":{"type":"Point","coordinates":[-150.0,61.0]}}
        ]}"#;
        let set = parse_features(FeatureFormat::GeoJson, json, None).unwrap();
        assert_eq!(set.value_field.as_deref(), Some("mag"));
        assert_eq!(set.value_range, Some((2.5, 6.1)));
        let chile = &set.points[0];
        assert_eq!(
            (chile.value, chile.time_ms),
            (Some(6.1), Some(1_793_700_000_000))
        );
        assert_eq!(chile.details, "tsunami: 1");
        // A CSV column named in the layer sizes the points.
        let csv = "city,lat,lon,people,founded\nLisbon,38.7,-9.1,545000,1147-01-01\n";
        let set = parse_features(FeatureFormat::Csv, csv.as_bytes(), Some("people")).unwrap();
        assert_eq!(set.points[0].value, Some(545_000.0));
        assert_eq!(set.points[0].details, "founded: 1147-01-01");
        // Without any value, no field is claimed.
        let plain = parse_features(FeatureFormat::Csv, b"lat,lon\n1,2\n", None).unwrap();
        assert_eq!((plain.value_field, plain.value_range), (None, None));
    }
}
