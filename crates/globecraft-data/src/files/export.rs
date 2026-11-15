//! Places and ruler paths written for other programs (roadmap 9): GeoJSON for data
//! tools and web maps, GPX for hiking apps and devices. KML stays the format places
//! are saved in; an export is a copy and does not become the open file.
use globecraft_core::placemark::{PlaceItem, PlaceNode, Placemark, Places};
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesText, Event},
};
use std::path::Path;

/// Decimals written for a coordinate: 1e-7° is about a centimetre.
const COORDINATE_DECIMALS: usize = 7;
/// Between the names of nested folders, in a place's `folder` property.
const FOLDER_SEPARATOR: &str = " / ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    GeoJson,
    Gpx,
}

impl ExportFormat {
    pub const ALL: [Self; 2] = [Self::GeoJson, Self::Gpx];
    pub fn code(self) -> &'static str {
        match self {
            Self::GeoJson => "geojson",
            Self::Gpx => "gpx",
        }
    }
    /// The file name ending, without the dot.
    pub fn extension(self) -> &'static str {
        self.code()
    }
    /// The name shown in a save dialog's format list.
    pub fn title(self) -> &'static str {
        match self {
            Self::GeoJson => "GeoJSON",
            Self::Gpx => "GPX",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.code().eq_ignore_ascii_case(code.trim()))
    }
    /// The format a file name asks for (`.geojson`, `.json` or `.gpx`).
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "geojson" | "json" => Some(Self::GeoJson),
            "gpx" => Some(Self::Gpx),
            _ => None,
        }
    }
}

/// A ruler path: a line, or a shape when `closed` (its last point joins its first).
#[derive(Clone, Debug, PartialEq)]
pub struct ExportPath {
    pub name: String,
    /// Latitude, longitude; a closed shape's first point is not repeated.
    pub points: Vec<(f64, f64)>,
    pub closed: bool,
}

/// What an export holds, captured when it is asked for.
#[derive(Clone, Debug, Default)]
pub struct ExportContent {
    /// The places tree; places and folders not shown (unticked) are left out.
    pub places: Places,
    pub paths: Vec<ExportPath>,
}

/// A place with the folders it lies in, outermost first.
struct Located<'a> {
    folders: Vec<&'a str>,
    place: &'a Placemark,
}

impl ExportContent {
    /// The shown places in tree order, each with its folders.
    fn located(&self) -> Vec<Located<'_>> {
        fn walk<'a>(
            nodes: &'a [PlaceNode],
            folders: &mut Vec<&'a str>,
            out: &mut Vec<Located<'a>>,
        ) {
            for node in nodes.iter().filter(|node| node.visible) {
                match &node.item {
                    PlaceItem::Marker(place) => out.push(Located {
                        folders: folders.clone(),
                        place,
                    }),
                    PlaceItem::Folder { name, children, .. } => {
                        folders.push(name);
                        walk(children, folders, out);
                        folders.pop();
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.places.roots, &mut Vec::new(), &mut out);
        out
    }

    /// The paths that draw something: two points or more.
    fn lines(&self) -> impl Iterator<Item = &ExportPath> {
        self.paths.iter().filter(|path| path.points.len() >= 2)
    }

    /// How many places and paths an export writes.
    pub fn counts(&self) -> (usize, usize) {
        (self.located().len(), self.lines().count())
    }

    pub fn is_empty(&self) -> bool {
        self.counts() == (0, 0)
    }

    /// The export as the bytes of a file in `format`.
    pub fn encode(&self, format: ExportFormat) -> Result<Vec<u8>, crate::Error> {
        for located in self.located() {
            globecraft_core::coordinates::validate_coordinates(
                located.place.lat,
                located.place.lon,
            )?;
        }
        for path in self.lines() {
            for &(lat, lon) in &path.points {
                globecraft_core::coordinates::validate_coordinates(lat, lon)?;
            }
        }
        match format {
            ExportFormat::GeoJson => self.geojson(),
            ExportFormat::Gpx => self.gpx(),
        }
    }

    fn geojson(&self) -> Result<Vec<u8>, crate::Error> {
        use serde_json::{Map, Value, json};
        let position = |lat: f64, lon: f64| json!([rounded(lon), rounded(lat)]);
        let mut features = Vec::new();
        for located in self.located() {
            let mut properties = Map::new();
            properties.insert("name".into(), located.place.name.clone().into());
            if !located.place.desc.is_empty() {
                properties.insert("description".into(), located.place.desc.clone().into());
            }
            if !located.folders.is_empty() {
                properties.insert(
                    "folder".into(),
                    located.folders.join(FOLDER_SEPARATOR).into(),
                );
            }
            features.push(json!({
                "type": "Feature",
                "properties": properties,
                "geometry": {
                    "type": "Point",
                    "coordinates": position(located.place.lat, located.place.lon),
                },
            }));
        }
        for path in self.lines() {
            let mut ring: Vec<(f64, f64)> = path.points.clone();
            let shape = path.closed && ring.len() >= 3;
            let geometry = if shape {
                // An outer ring runs anticlockwise (RFC 7946) and ends where it began.
                if signed_area(&ring) < 0.0 {
                    ring.reverse();
                }
                ring.push(ring[0]);
                let ring: Vec<Value> = ring.iter().map(|&(lat, lon)| position(lat, lon)).collect();
                json!({"type": "Polygon", "coordinates": [ring]})
            } else {
                let line: Vec<Value> = ring.iter().map(|&(lat, lon)| position(lat, lon)).collect();
                json!({"type": "LineString", "coordinates": line})
            };
            features.push(json!({
                "type": "Feature",
                "properties": {"name": path.name},
                "geometry": geometry,
            }));
        }
        let mut bytes = serde_json::to_vec_pretty(&json!({
            "type": "FeatureCollection",
            "features": features,
        }))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn gpx(&self) -> Result<Vec<u8>, crate::Error> {
        let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        writer
            .create_element("gpx")
            .with_attributes([
                ("version", "1.1"),
                ("creator", "GlobeCraft"),
                ("xmlns", "http://www.topografix.com/GPX/1/1"),
            ])
            .write_inner_content(|writer| {
                // The schema's order: waypoints, then routes.
                for located in self.located() {
                    let (lat, lon) = (coordinate(located.place.lat), coordinate(located.place.lon));
                    writer
                        .create_element("wpt")
                        .with_attributes([("lat", lat.as_str()), ("lon", lon.as_str())])
                        .write_inner_content(|writer| {
                            text_element(writer, "name", &located.place.name)?;
                            if !located.place.desc.is_empty() {
                                text_element(writer, "desc", &located.place.desc)?;
                            }
                            if !located.folders.is_empty() {
                                text_element(
                                    writer,
                                    "type",
                                    &located.folders.join(FOLDER_SEPARATOR),
                                )?;
                            }
                            Ok(())
                        })?;
                }
                for path in self.lines() {
                    writer.create_element("rte").write_inner_content(|writer| {
                        text_element(writer, "name", &path.name)?;
                        let closing =
                            (path.closed && path.points.len() >= 3).then(|| path.points[0]);
                        for &(lat, lon) in path.points.iter().chain(closing.iter()) {
                            let (lat, lon) = (coordinate(lat), coordinate(lon));
                            writer
                                .create_element("rtept")
                                .with_attributes([("lat", lat.as_str()), ("lon", lon.as_str())])
                                .write_empty()?;
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
        let mut bytes = writer.into_inner();
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// `<name>text</name>`, without the characters XML 1.0 cannot hold.
fn text_element(writer: &mut Writer<Vec<u8>>, name: &str, text: &str) -> std::io::Result<()> {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        .collect();
    writer
        .create_element(name)
        .write_text_content(BytesText::new(&clean))?;
    Ok(())
}

/// A coordinate as text: fixed decimals without trailing zeros (`48.1372`, `11`).
fn coordinate(degrees: f64) -> String {
    let text = format!("{degrees:.COORDINATE_DECIMALS$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" | "" => "0".to_owned(),
        text => text.to_owned(),
    }
}

/// A coordinate as a number with the same decimals as [`coordinate`].
fn rounded(degrees: f64) -> f64 {
    let scale = 10f64.powi(COORDINATE_DECIMALS as i32);
    let value = (degrees * scale).round() / scale;
    if value == 0.0 { 0.0 } else { value }
}

/// Twice the area of a ring in the longitude/latitude plane, positive anticlockwise
/// (the shoelace sum). Only its sign is used.
fn signed_area(ring: &[(f64, f64)]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&(lat_a, lon_a), &(lat_b, lon_b))| lon_a * lat_b - lon_b * lat_a)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_data::{FeatureFormat, parse_features};

    fn place(name: &str, desc: &str, lat: f64, lon: f64) -> PlaceNode {
        PlaceNode::marker(Placemark {
            name: name.into(),
            desc: desc.into(),
            lat,
            lon,
        })
    }

    fn content() -> ExportContent {
        let mut hidden = place("Hidden", "", 1.0, 1.0);
        hidden.visible = false;
        let mut hidden_folder = PlaceNode::folder(
            "Archive".into(),
            String::new(),
            false,
            vec![place("Old", "", 2.0, 2.0)],
        );
        hidden_folder.visible = false;
        let trip = PlaceNode::folder(
            "Trip".into(),
            String::new(),
            false,
            vec![
                place(
                    "Hut <A> & \"B\"",
                    "Open May\u{b}–Oct",
                    46.558_123_456_7,
                    8.561,
                ),
                PlaceNode::folder(
                    "Day 2".into(),
                    String::new(),
                    false,
                    vec![place("Pass", "", 46.6, 8.6)],
                ),
            ],
        );
        ExportContent {
            places: Places {
                roots: vec![
                    place("Zürich", "", 47.37, 8.54),
                    hidden,
                    hidden_folder,
                    trip,
                ],
            },
            paths: vec![
                ExportPath {
                    name: "Ruler path 1".into(),
                    points: vec![(46.0, 8.0), (46.5, 8.5), (47.0, 8.0)],
                    closed: false,
                },
                // Drawn clockwise; a shape.
                ExportPath {
                    name: "Ruler shape 2".into(),
                    points: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
                    closed: true,
                },
                // One point draws nothing.
                ExportPath {
                    name: "Ruler path 3".into(),
                    points: vec![(5.0, 5.0)],
                    closed: false,
                },
            ],
        }
    }

    #[test]
    fn geojson_holds_the_shown_places_with_their_folders_and_the_paths() {
        let content = content();
        assert_eq!(content.counts(), (3, 2));
        let bytes = content.encode(ExportFormat::GeoJson).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let features = json["features"].as_array().unwrap();
        let names: Vec<&str> = features
            .iter()
            .map(|f| f["properties"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "Zürich",
                "Hut <A> & \"B\"",
                "Pass",
                "Ruler path 1",
                "Ruler shape 2"
            ]
        );
        // Longitude first, a centimetre's decimals.
        assert_eq!(
            features[1]["geometry"]["coordinates"],
            serde_json::json!([8.561, 46.558_123_5])
        );
        assert!(features[0]["properties"].get("folder").is_none());
        assert_eq!(features[1]["properties"]["folder"], "Trip");
        assert_eq!(features[2]["properties"]["folder"], "Trip / Day 2");
        assert_eq!(features[3]["geometry"]["type"], "LineString");
        // The shape: a closed ring, turned anticlockwise.
        let ring = features[4]["geometry"]["coordinates"][0]
            .as_array()
            .unwrap();
        assert_eq!(ring.len(), 5);
        assert_eq!(ring[0], ring[4]);
        let points: Vec<(f64, f64)> = ring[..4]
            .iter()
            .map(|p| (p[1].as_f64().unwrap(), p[0].as_f64().unwrap()))
            .collect();
        assert!(signed_area(&points) > 0.0);
        // GlobeCraft reads its own export back as a layer.
        let read = parse_features(FeatureFormat::GeoJson, &bytes, None).unwrap();
        assert_eq!((read.points.len(), read.lines.len()), (3, 2));
    }

    #[test]
    fn gpx_holds_waypoints_and_routes_and_reads_back() {
        let bytes = content().encode(ExportFormat::Gpx).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\"")
        );
        assert!(
            text.contains("<wpt lat=\"46.5581235\" lon=\"8.561\">"),
            "{text}"
        );
        assert!(
            text.contains("<name>Hut &lt;A&gt; &amp; &quot;B&quot;</name>"),
            "{text}"
        );
        // The vertical tab XML cannot hold is left out.
        assert!(text.contains("<desc>Open May–Oct</desc>"), "{text}");
        assert!(text.contains("<type>Trip / Day 2</type>"), "{text}");
        assert!(!text.contains("Hidden") && !text.contains("Old"));
        // Waypoints come before routes, as the schema asks.
        assert!(text.rfind("<wpt").unwrap() < text.find("<rte>").unwrap());
        let read = parse_features(FeatureFormat::Gpx, &bytes, None).unwrap();
        let names: Vec<&str> = read.points.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Zürich", "Hut <A> & \"B\"", "Pass"]);
        assert_eq!(read.lines.len(), 2);
        assert_eq!(read.lines[0], [(46.0, 8.0), (46.5, 8.5), (47.0, 8.0)]);
        // The shape's route ends back at its first point.
        assert_eq!(read.lines[1].len(), 5);
        assert_eq!(read.lines[1][0], read.lines[1][4]);
    }

    #[test]
    fn formats_follow_names_and_empty_or_invalid_exports_are_refused() {
        assert_eq!(
            ExportFormat::from_path(Path::new("/tmp/Trip.GPX")),
            Some(ExportFormat::Gpx)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("places.geojson")),
            Some(ExportFormat::GeoJson)
        );
        assert_eq!(ExportFormat::from_path(Path::new("places.kml")), None);
        assert_eq!(ExportFormat::from_code(" GPX "), Some(ExportFormat::Gpx));
        assert!(ExportContent::default().is_empty());
        let invalid = ExportContent {
            places: Places {
                roots: vec![place("Nowhere", "", 95.0, 0.0)],
            },
            paths: Vec::new(),
        };
        assert!(invalid.encode(ExportFormat::Gpx).is_err());
        assert_eq!(coordinate(-0.000_000_01), "0");
        assert_eq!(coordinate(11.0), "11");
        assert_eq!(coordinate(-122.419_4), "-122.4194");
    }
}
