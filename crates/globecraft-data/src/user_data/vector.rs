//! Features of a user layer: points with their names, and lines (tracks, routes and
//! the outlines of areas), read from GeoJSON, GPX or CSV with coordinate columns, from
//! a file or a URL. A URL that is a live feed is read again at its interval.
use super::FeatureFormat;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The largest file read.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// More vertices than this are left out (the layer says so).
const MAX_VERTICES: usize = 2_000_000;
/// A failed load of a URL is tried again after this long.
const RETRY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct NamedPoint {
    pub lat: f64,
    pub lon: f64,
    pub name: String,
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
}

impl FeatureSet {
    fn vertices(&self) -> usize {
        self.points.len() + self.lines.iter().map(Vec::len).sum::<usize>()
    }
    fn room(&self) -> bool {
        self.vertices() < MAX_VERTICES
    }
    fn point(&mut self, lat: f64, lon: f64, name: String) {
        if !valid(lat, lon) {
            return;
        }
        if !self.room() {
            self.truncated = true;
            return;
        }
        self.points.push(NamedPoint { lat, lon, name });
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
        self
    }
}

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

/// Reads features in `format` from `bytes`.
pub fn parse_features(format: FeatureFormat, bytes: &[u8]) -> Result<FeatureSet, crate::Error> {
    let set = match format {
        FeatureFormat::GeoJson => geojson(bytes)?,
        FeatureFormat::Gpx => gpx(bytes)?,
        FeatureFormat::Csv => csv(bytes)?,
    }
    .finish();
    if set.points.is_empty() && set.lines.is_empty() {
        return Err("no points or lines with coordinates were found".into());
    }
    Ok(set)
}

// ---- GeoJSON ----

fn geojson(bytes: &[u8]) -> Result<FeatureSet, crate::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let mut set = FeatureSet::default();
    geojson_object(&value, "", &mut set);
    Ok(set)
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

fn geojson_object(value: &serde_json::Value, name: &str, set: &mut FeatureSet) {
    let kind = value.get("type").and_then(serde_json::Value::as_str);
    match kind {
        Some("FeatureCollection") => {
            for feature in value
                .get("features")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                geojson_object(feature, "", set);
            }
        }
        Some("Feature") => {
            let name = feature_name(value.get("properties"));
            if let Some(geometry) = value.get("geometry") {
                geojson_object(geometry, &name, set);
            }
        }
        Some("GeometryCollection") => {
            for geometry in value
                .get("geometries")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                geojson_object(geometry, name, set);
            }
        }
        Some(kind) => {
            let Some(coordinates) = value.get("coordinates") else {
                return;
            };
            match kind {
                "Point" => {
                    if let Some((lat, lon)) = position(coordinates) {
                        set.point(lat, lon, name.to_owned());
                    }
                }
                "MultiPoint" => {
                    for (lat, lon) in positions(coordinates) {
                        set.point(lat, lon, name.to_owned());
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
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
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
                    set.point(lat, lon, String::new());
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
            Event::End(element) => match element.local_name().as_ref() {
                "name" => in_name = false,
                "wpt" => {
                    if let Some((lat, lon, _)) = point.take() {
                        set.point(lat, lon, std::mem::take(&mut name));
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
        set.point(lat, lon, name);
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

fn csv(bytes: &[u8]) -> Result<FeatureSet, crate::Error> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("the file is empty")?;
    let delimiter = [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|d| header.matches(*d).count())
        .unwrap_or(',');
    let columns: Vec<String> = split(header, delimiter)
        .into_iter()
        .map(|c| c.trim().trim_matches('"').to_ascii_lowercase())
        .collect();
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
            fields.get(i).and_then(|f| {
                f.trim()
                    .trim_matches('"')
                    .replace(',', ".")
                    .parse::<f64>()
                    .ok()
            })
        };
        if let (Some(la), Some(lo)) = (number(lat), number(lon)) {
            let label = name
                .and_then(|i| fields.get(i))
                .map(|f| f.trim().trim_matches('"').to_owned())
                .unwrap_or_default();
            set.point(la, lo, label);
        }
    }
    Ok(set)
}

/// A CSV line's fields, honouring double quotes.
fn split(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                field.push(c);
            }
            c if c == delimiter && !quoted => fields.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    fields.push(field);
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
    refresh: Option<Duration>,
    shared: Arc<Mutex<Shared>>,
}

impl VectorLayer {
    pub fn new(location: &str, format: FeatureFormat, refresh_minutes: u32) -> Self {
        Self {
            location: location.trim().to_owned(),
            format,
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
        runtime.spawn(async move {
            let result = load(location, format).await;
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
pub async fn load(location: String, format: FeatureFormat) -> Result<FeatureSet, crate::Error> {
    let bytes = if super::is_url(&location) {
        let client = crate::http::client(crate::http::Policy::UserData)?;
        crate::http::body(client.get(&location), MAX_BYTES).await?
    } else {
        crate::cpu::run(crate::cpu::Service::Files, move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(&location)?
                .take(MAX_BYTES)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .await?
    };
    crate::cpu::run(crate::cpu::Service::Features, move || {
        parse_features(format, &bytes)
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
        let set = parse_features(FeatureFormat::GeoJson, json).unwrap();
        assert_eq!(
            set.points,
            [NamedPoint {
                lat: 38.1,
                lon: 142.3,
                name: "M 4.6 - Japan".into()
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
        let set = parse_features(FeatureFormat::Gpx, gpx).unwrap();
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
        let set = parse_features(FeatureFormat::Csv, csv.as_bytes()).unwrap();
        assert_eq!(
            set.points,
            [NamedPoint {
                lat: 38.72,
                lon: -9.14,
                name: "Lisbon, PT".into()
            }]
        );
        let error = parse_features(FeatureFormat::Csv, b"a,b\n1,2\n").unwrap_err();
        assert!(error.to_string().contains("latitude"), "{error}");
    }
}
