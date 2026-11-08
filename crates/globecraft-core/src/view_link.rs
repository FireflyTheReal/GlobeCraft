//! Links to a view (roadmap 2.6): `globecraft://view?lat=…&lon=…`, carrying the camera,
//! optionally the basemap, the layers shown and web data to add. They are written
//! by "Copy View Link", opened from the search box, Command Search or the command
//! line, and are plain text a person can read and edit.
//!
//! `globecraft://view?lat=48.13720&lon=11.57540&alt=12.5&heading=30&tilt=45`
//! `&basemap=satellite&layers=country_borders,time_zones&data=https%3A%2F%2F…`

/// The scheme and the one kind of link so far.
pub const PREFIX: &str = "globecraft://view";

/// What a view link carries. Only the position is required.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewLink {
    pub lat: f64,
    pub lon: f64,
    pub altitude_km: Option<f64>,
    pub heading_deg: Option<f64>,
    pub tilt_deg: Option<f64>,
    /// A basemap code (`satellite`, `streets`…), checked by the app.
    pub basemap: Option<String>,
    /// Layer codes to show; every other layer of the map's content is hidden. None
    /// leaves the layers as they are.
    pub layers: Option<Vec<String>>,
    /// Web data to add (tile templates, WMS addresses, GeoJSON/GPX/CSV files).
    pub data: Vec<String>,
}

impl ViewLink {
    /// The link as text, with five decimals for the position (about a metre).
    pub fn to_link(&self) -> String {
        let mut link = format!("{PREFIX}?lat={:.5}&lon={:.5}", self.lat, self.lon);
        let mut number = |key: &str, value: Option<f64>, decimals: usize| {
            if let Some(value) = value {
                let text = format!("{value:.decimals$}");
                let text = if text.contains('.') {
                    text.trim_end_matches('0').trim_end_matches('.')
                } else {
                    &text
                };
                link.push_str(&format!("&{key}={text}"));
            }
        };
        number("alt", self.altitude_km, 3);
        number("heading", self.heading_deg, 1);
        number("tilt", self.tilt_deg, 1);
        if let Some(basemap) = &self.basemap {
            link.push_str(&format!("&basemap={}", encode(basemap)));
        }
        if let Some(layers) = &self.layers {
            let codes: Vec<String> = layers.iter().map(|code| encode(code)).collect();
            link.push_str(&format!("&layers={}", codes.join(",")));
        }
        for data in &self.data {
            link.push_str(&format!("&data={}", encode(data)));
        }
        link
    }

    /// Reads a link. Unknown parameters are ignored, so newer links still open.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let rest = text
            .strip_prefix(PREFIX)
            .ok_or_else(|| format!("a view link starts with {PREFIX}"))?;
        let query = rest
            .strip_prefix('/')
            .unwrap_or(rest)
            .strip_prefix('?')
            .ok_or("the link has no position (lat and lon)")?;
        let mut link = ViewLink::default();
        let (mut lat, mut lon) = (None, None);
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = decode(value)?;
            let number = || -> Result<f64, String> {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("{key} is not a number: {value}"))
            };
            match key {
                "lat" => lat = Some(number()?),
                "lon" => lon = Some(number()?),
                "alt" => link.altitude_km = Some(number()?),
                "heading" => link.heading_deg = Some(number()?),
                "tilt" => link.tilt_deg = Some(number()?),
                "basemap" => link.basemap = Some(value),
                "layers" => {
                    link.layers = Some(
                        value
                            .split(',')
                            .filter(|code| !code.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    )
                }
                "data" if !value.is_empty() => link.data.push(value),
                _ => {}
            }
        }
        link.lat = lat.ok_or("the link has no latitude (lat)")?;
        link.lon = lon.ok_or("the link has no longitude (lon)")?;
        if !(-90.0..=90.0).contains(&link.lat) || !(-180.0..=180.0).contains(&link.lon) {
            return Err(format!(
                "the position {}, {} is not on the Earth",
                link.lat, link.lon
            ));
        }
        Ok(link)
    }
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Undoes [`encode`] (and `+` for a space, as forms write it).
fn decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = text
                    .get(i + 1..i + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                    .ok_or_else(|| format!("bad escape in the link: {text}"))?;
                out.push(hex);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| format!("the link is not valid text: {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_link_round_trips() {
        let view = ViewLink {
            lat: 48.137_2,
            lon: 11.575_4,
            altitude_km: Some(12.5),
            heading_deg: Some(30.0),
            tilt_deg: Some(45.26),
            basemap: Some("satellite".into()),
            layers: Some(vec!["country_borders".into(), "time_zones".into()]),
            data: vec!["https://example.org/tiles/{z}/{x}/{y}.png?key=a b&c".into()],
        };
        let link = view.to_link();
        assert!(link.starts_with(
            "globecraft://view?lat=48.13720&lon=11.57540&alt=12.5&heading=30&tilt=45.3"
        ));
        assert!(!link[PREFIX.len()..].contains(' ') && !link.contains("{z}"));
        let back = ViewLink::parse(&link).unwrap();
        assert_eq!(back.data, view.data);
        assert_eq!(back.layers, view.layers);
        assert_eq!(back.altitude_km, Some(12.5));
        assert!((back.lat - 48.1372).abs() < 1e-9);
    }

    #[test]
    fn links_need_a_position_on_the_earth_and_ignore_what_they_do_not_know() {
        let only = ViewLink::parse("  globecraft://view/?lon=2.35&lat=48.85&future=1 ").unwrap();
        assert_eq!((only.lat, only.lon, only.altitude_km), (48.85, 2.35, None));
        assert!(only.layers.is_none() && only.data.is_empty());
        assert!(ViewLink::parse("globecraft://view?lat=48").is_err());
        assert!(ViewLink::parse("globecraft://view?lat=91&lon=0").is_err());
        assert!(ViewLink::parse("globecraft://view?lat=x&lon=0").is_err());
        assert!(ViewLink::parse("https://example.org/?lat=1&lon=2").is_err());
        assert!(ViewLink::parse("globecraft://view?lat=1&lon=2&data=%G0").is_err());
    }
}
