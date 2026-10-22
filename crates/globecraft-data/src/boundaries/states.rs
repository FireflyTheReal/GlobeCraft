//! State and province lines, streamed ahead of the altitude where they appear.
//!
//! The lines come from Natural Earth's admin-1 *boundary lines*, not its polygons:
//! each shared border is stored once and coastlines are not outlined, which cuts the
//! 1:10m layer from 1.3 M to 0.4 M vertices. Names come from separate label points.
//! Loading is independent of the world detail level and starts above the visible
//! band (scaled from the user's "States visible below" setting), so the lines are
//! ready before they fade in.
use super::geojson::{collect_geojson_lines, feature_name, label_center};
use super::sources::{FeatureSource, first_features, natural_earth};
use super::{
    LabelKind, Layer, Line, MapLabel, Patch, STATE_BIG_VISIBLE_MAX_ALTITUDE_KM, StatusLog,
    is_big_country, log_status,
};

/// Loading starts this many times above the altitude where a layer becomes visible,
/// so a download of a few seconds finishes while the camera is still descending.
pub(super) const STATE_PREFETCH_FACTOR: f64 = 1.6;

/// Which state data the camera needs. Ordered: a later stage includes the earlier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum StateStage {
    None = 0,
    /// Provinces of the largest countries (1:50m covers exactly those).
    Big = 1,
    /// Every country's states and provinces (1:10m).
    All = 2,
}

impl StateStage {
    /// The stage for a camera altitude and the user's state visibility altitude.
    pub(super) fn wanted(altitude_km: f64, state_visible_max_km: f64) -> Self {
        if !altitude_km.is_finite() {
            return Self::None;
        }
        // Regular states show below the user's setting; big-country provinces
        // fade in from their own fixed altitude and use 1:10m once close enough.
        let all_from = state_visible_max_km.max(0.0) * STATE_PREFETCH_FACTOR;
        if altitude_km <= all_from {
            Self::All
        } else if altitude_km <= STATE_BIG_VISIBLE_MAX_ALTITUDE_KM * STATE_PREFETCH_FACTOR {
            Self::Big
        } else {
            Self::None
        }
    }

    pub(super) fn from_code(code: u8) -> Self {
        match code {
            2 => Self::All,
            1 => Self::Big,
            _ => Self::None,
        }
    }
}

/// Admin-1 line features that are real state/province borders.
///
/// The 1:10m file stores each border once and splits them into classes: besides
/// "Admin-1 boundary", the region, statistical and "statistical meta" classes are
/// also state borders (Texas–New Mexico, French regions, Russian oblasts, Chinese
/// provinces); keeping only the first class dropped about a fifth of all state
/// borders. Only the short "indicator" ticks and unrecognized (disputed) lines are
/// left out.
fn is_state_border(feature: &serde_json::Value) -> bool {
    let class = feature
        .get("properties")
        .and_then(|p| p.get("FEATURECLA").or_else(|| p.get("featurecla")))
        .and_then(|v| v.as_str());
    !matches!(
        class,
        Some("Admin-1 boundary indicator" | "Unrecognized Admin-1 boundary")
    )
}

#[derive(Default)]
struct StateLines {
    lines: Vec<Line>,
    lines_big: Vec<Line>,
}

fn line_sources(stage: StateStage) -> Vec<FeatureSource> {
    match stage {
        StateStage::None => Vec::new(),
        StateStage::Big => vec![natural_earth("ne_50m_admin_1_states_provinces_lines")],
        StateStage::All => vec![
            natural_earth("ne_10m_admin_1_states_provinces_lines"),
            natural_earth("ne_50m_admin_1_states_provinces_lines"),
        ],
    }
}

fn label_sources(stage: StateStage) -> Vec<FeatureSource> {
    match stage {
        StateStage::None => Vec::new(),
        // The 1:50m polygons are small and name exactly the countries 1:50m lines cover.
        StateStage::Big => vec![natural_earth("ne_50m_admin_1_states_provinces")],
        StateStage::All => vec![
            natural_earth("ne_10m_admin_1_label_points_details"),
            natural_earth("ne_50m_admin_1_states_provinces"),
        ],
    }
}

fn collect_state_line(feature: &serde_json::Value, out: &mut StateLines) {
    if !is_state_border(feature) {
        return;
    }
    let Some(coords) = feature.pointer("/geometry/coordinates") else {
        return;
    };
    let target = if is_big_country(feature) {
        &mut out.lines_big
    } else {
        &mut out.lines
    };
    collect_geojson_lines(coords, target);
}

const EN_KEYS: &[&str] = &["name_en", "name", "NAME_1", "gn_name"];
const PT_KEYS: &[&str] = &["name_pt", "name", "name_en", "NAME_1"];

fn state_label(feature: &serde_json::Value) -> Option<MapLabel> {
    let geometry = feature.get("geometry").filter(|g| !g.is_null())?;
    let (lat, lon) = match geometry.get("type").and_then(|t| t.as_str()) {
        Some("Point") => crate::json_fields::coordinates(geometry.get("coordinates")?)?,
        _ => label_center(geometry)?,
    };
    let en = feature_name(feature, EN_KEYS)?;
    let pt = feature_name(feature, PT_KEYS).unwrap_or_else(|| en.clone());
    Some(MapLabel {
        lat,
        lon,
        en,
        pt,
        kind: LabelKind::State,
        rank: 0,
        big: is_big_country(feature),
    })
}

/// Loads one stage as a replacing `States` patch, or `None` when every source failed.
pub(super) async fn load_stage(log: &StatusLog, stage: StateStage) -> Option<Patch> {
    let started = std::time::Instant::now();
    let lines = first_features(
        log,
        "State borders",
        &line_sources(stage),
        collect_state_line,
    )
    .await?;
    let labels = first_features(
        log,
        "State names",
        &label_sources(stage),
        |feature: &serde_json::Value, out: &mut Vec<MapLabel>| {
            out.extend(state_label(feature));
        },
    )
    .await
    .unwrap_or_default();
    log_status(
        log,
        format!(
            "✔ States ({}): {} lines, {} names in {} ms",
            match stage {
                StateStage::Big => "largest countries",
                _ => "all countries",
            },
            lines.lines.len() + lines.lines_big.len(),
            labels.len(),
            started.elapsed().as_millis()
        ),
    );
    Some(Patch {
        layer: Layer::States,
        lines: lines.lines,
        lines_big: lines.lines_big,
        labels,
        regions: vec![],
        replace: true,
        municipal: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_start_ahead_of_the_visible_band_and_follow_the_setting() {
        // Default setting 1,609 km: everything loads from about 2,570 km.
        assert_eq!(StateStage::wanted(2_500.0, 1_609.0), StateStage::All);
        assert_eq!(StateStage::wanted(3_000.0, 1_609.0), StateStage::Big);
        // Big-country provinces appear below 4,000 km and load from 6,400 km.
        assert_eq!(StateStage::wanted(6_300.0, 1_609.0), StateStage::Big);
        assert_eq!(StateStage::wanted(7_000.0, 1_609.0), StateStage::None);
        // A higher user setting moves the full load up with it.
        assert_eq!(StateStage::wanted(7_000.0, 5_000.0), StateStage::All);
        assert_eq!(StateStage::wanted(f64::NAN, 1_609.0), StateStage::None);
        assert!(StateStage::Big < StateStage::All);
        for stage in [StateStage::None, StateStage::Big, StateStage::All] {
            assert_eq!(StateStage::from_code(stage as u8), stage);
        }
    }

    #[test]
    fn only_real_state_borders_are_kept() {
        let line = |class: &str| serde_json::json!({"properties": {"FEATURECLA": class}});
        for class in [
            "Admin-1 boundary",
            "Admin-1 region boundary",
            "Admin-1 statistical boundary",
            "Admin-1 statistical meta bounds",
        ] {
            assert!(is_state_border(&line(class)), "{class}");
        }
        assert!(!is_state_border(&line("Admin-1 boundary indicator")));
        assert!(!is_state_border(&line("Unrecognized Admin-1 boundary")));
        assert!(is_state_border(&serde_json::json!({"properties": {}})));
    }

    #[test]
    fn label_points_and_polygons_both_name_states() {
        let point = serde_json::json!({
            "properties": {"name_en": "Bavaria", "name_pt": "Baviera", "adm0_a3": "DEU"},
            "geometry": {"type": "Point", "coordinates": [11.4, 48.9]}
        });
        let label = state_label(&point).unwrap();
        assert_eq!(
            (label.en.as_str(), label.pt.as_str()),
            ("Bavaria", "Baviera")
        );
        assert_eq!((label.lat, label.lon), (48.9, 11.4));
        assert!(!label.big);
        let polygon = serde_json::json!({
            "properties": {"name": "Texas", "adm0_a3": "USA"},
            "geometry": {"type": "Polygon", "coordinates": [[[-100.0, 30.0], [-96.0, 30.0], [-96.0, 34.0], [-100.0, 30.0]]]}
        });
        let label = state_label(&polygon).unwrap();
        assert_eq!(label.pt, "Texas");
        assert!(label.big);
    }

    #[test]
    #[ignore = "manual: reads Natural Earth files from GLOBECRAFT_NE_DIR"]
    fn natural_earth_files_parse() {
        let dir = std::env::var("GLOBECRAFT_NE_DIR").unwrap();
        let read = |name: &str| std::fs::read(format!("{dir}/{name}.geojson")).unwrap();
        let lines = crate::features::project(
            read("ne_10m_admin_1_states_provinces_lines").as_slice(),
            collect_state_line,
        )
        .unwrap();
        let labels = crate::features::project(
            read("ne_10m_admin_1_label_points_details").as_slice(),
            |f: &serde_json::Value, out: &mut Vec<MapLabel>| out.extend(state_label(f)),
        )
        .unwrap();
        let vertices: usize = lines
            .lines
            .iter()
            .chain(&lines.lines_big)
            .map(|l| l.len())
            .sum();
        println!(
            "10m lines: {} regular + {} big, {vertices} vertices; {} labels ({} big), e.g. {:?}",
            lines.lines.len(),
            lines.lines_big.len(),
            labels.len(),
            labels.iter().filter(|l| l.big).count(),
            labels
                .iter()
                .find(|l| l.en == "Bavaria")
                .map(|l| (&l.pt, l.lat, l.lon))
        );
        assert!(lines.lines.len() > 1000 && lines.lines_big.len() > 100);
    }

    #[test]
    fn line_sources_prefer_the_lines_files() {
        assert!(line_sources(StateStage::None).is_empty());
        let all = line_sources(StateStage::All);
        assert!(
            matches!(&all[0], FeatureSource::Url(url) if url.ends_with("ne_10m_admin_1_states_provinces_lines.geojson"))
        );
    }
}
