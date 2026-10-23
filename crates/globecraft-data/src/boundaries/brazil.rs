use super::geojson::{collect_geojson_lines, feature_name, label_center};
use super::{
    Detail, LOCAL_BIG_LABEL_RANK, LabelKind, Layer, Line, MapLabel, Patch, StatusLog, log_status,
};
use crate::http::fetch_json;
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
};
const IBGE: &str = "https://servicodados.ibge.gov.br/api";
const BR_UFS: [u32; 27] = [
    11, 12, 13, 14, 15, 16, 17, 21, 22, 23, 24, 25, 26, 27, 28, 29, 31, 32, 33, 35, 41, 42, 43, 50,
    51, 52, 53,
];

struct Muni {
    name: String,
    lines: Vec<Line>,
    center: Option<(f64, f64)>,
}

fn json_code(v: Option<&serde_json::Value>) -> Option<u32> {
    let v = v?;
    v.as_u64()
        .map(|n| n as u32)
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

async fn load_brazil_state(uf: u32, include_geometry: bool) -> Result<Vec<Muni>, crate::Error> {
    let mut names: HashMap<u32, String> = HashMap::new();
    if let Ok(v) = fetch_json(&format!("{IBGE}/v1/localidades/estados/{uf}/municipios")).await {
        for m in v.as_array().into_iter().flatten() {
            if let (Some(id), Some(n)) = (
                json_code(m.get("id")),
                m.get("nome").and_then(|n| n.as_str()),
            ) {
                names.insert(id, n.to_owned());
            }
        }
    }
    let urls = [
        format!(
            "{IBGE}/v3/malhas/estados/{uf}?intrarregiao=municipio&qualidade=intermediaria&formato=application/vnd.geo+json"
        ),
        format!(
            "{IBGE}/v3/malhas/estados/{uf}?intrarregiao=municipio&qualidade=minima&formato=application/vnd.geo+json"
        ),
        format!(
            "https://raw.githubusercontent.com/tbrugz/geodata-br/master/geojson/geojs-{uf}-mun.json"
        ),
    ];
    let names = Arc::new(names);
    let mut last = String::new();
    for url in &urls {
        let names = names.clone();
        let result =
            crate::features::fetch(url, move |f: &serde_json::Value, out: &mut Vec<Muni>| {
                let code = json_code(f.get("properties").and_then(|p| {
                    p.get("codarea")
                        .or_else(|| p.get("id"))
                        .or_else(|| p.get("CD_MUN"))
                }));
                let Some(code) = code else {
                    return;
                };
                let Some(geometry) = f.get("geometry") else {
                    return;
                };
                let mut lines = Vec::new();
                if include_geometry && let Some(coords) = geometry.get("coordinates") {
                    collect_geojson_lines(coords, &mut lines);
                }
                let name = names
                    .get(&code)
                    .cloned()
                    .or_else(|| feature_name(f, &["name", "NM_MUN", "nome"]))
                    .unwrap_or_default();
                out.push(Muni {
                    name,
                    lines,
                    center: label_center(geometry),
                });
            })
            .await;
        match result {
            Ok(out) if !out.is_empty() => return Ok(out),
            Ok(_) => last = "no municipalities in the document".into(),
            Err(error) => last = error.to_string(),
        }
    }
    Err(format!("State {uf}: {last}").into())
}

/// One state's municipalities (IBGE) as outlines and names, for the municipal streamer.
pub(super) async fn load_state_municipalities(uf: u32) -> Result<Detail, crate::Error> {
    let munis = load_brazil_state(uf, true).await?;
    let mut detail = Detail::default();
    for m in munis {
        detail.features += 1;
        if let (false, Some((lat, lon))) = (m.name.is_empty(), m.center) {
            detail.labels.push(MapLabel {
                lat,
                lon,
                en: m.name.clone(),
                pt: m.name,
                kind: LabelKind::City,
                rank: LOCAL_BIG_LABEL_RANK,
                big: false,
            });
        }
        detail.lines.extend(m.lines);
    }
    // One state per load: its municipalities share the state as parent.
    detail.parents = vec![Some(u64::from(uf)); detail.lines.len()];
    Ok(detail)
}

/// Approximate bounds of each Brazilian state (IBGE code, min lat, min lon, max lat,
/// max lon), so the streamer loads only the states around the view.
pub(super) const BR_UF_BOUNDS: [(u32, f64, f64, f64, f64); 27] = [
    (11, -13.7, -66.9, -7.9, -59.8),
    (12, -11.2, -74.0, -7.1, -66.6),
    (13, -9.9, -73.8, 2.3, -56.1),
    (14, -1.6, -64.9, 5.3, -58.9),
    (15, -9.9, -58.9, 2.6, -46.0),
    (16, -1.3, -54.9, 4.5, -49.8),
    (17, -13.5, -50.8, -5.2, -45.7),
    (21, -10.3, -48.8, -1.0, -41.8),
    (22, -10.9, -46.0, -2.7, -40.4),
    (23, -7.9, -41.5, -2.7, -37.2),
    (24, -7.0, -38.6, -4.8, -34.9),
    (25, -8.3, -38.8, -6.0, -34.8),
    (26, -9.5, -41.4, -3.8, -32.3),
    (27, -10.5, -38.3, -8.8, -35.1),
    (28, -11.6, -38.3, -9.5, -36.4),
    (29, -18.4, -46.7, -8.5, -37.3),
    (31, -23.0, -51.1, -14.2, -39.8),
    (32, -21.3, -41.9, -17.9, -39.6),
    (33, -23.4, -44.9, -20.7, -40.9),
    (35, -25.4, -53.2, -19.7, -44.1),
    (41, -26.8, -54.7, -22.5, -48.0),
    (42, -29.4, -53.9, -25.9, -48.3),
    (43, -33.8, -57.7, -27.0, -49.6),
    (50, -24.1, -58.2, -17.1, -50.9),
    (51, -18.1, -61.7, -7.3, -50.2),
    (52, -19.5, -53.3, -12.3, -45.9),
    (53, -16.1, -48.3, -15.5, -47.3),
];

// Brazil: municipality names from IBGE (official API, GitHub mirror as fallback).
pub(super) async fn load_brazil(log: &StatusLog, tx: &mpsc::Sender<Patch>, include_geometry: bool) {
    // One state at a time bounds large JSON/geometry allocations.
    let mut results = Vec::new();
    for uf in BR_UFS {
        results.push(load_brazil_state(uf, include_geometry).await);
    }
    let mut munis: Vec<Muni> = Vec::new();
    let mut errs = Vec::new();
    for r in results {
        match r {
            Ok(m) => munis.extend(m),
            Err(e) => errs.push(e.to_string()),
        }
    }
    if !errs.is_empty() {
        log_status(log, format!("✖ Municipalities: {}", errs.join("; ")));
    }
    if munis.is_empty() {
        return;
    }

    // Outlines stream per state through the municipal streamer; this whole-country
    // pass only supplies names.
    let mut labels = Vec::new();
    for m in &munis {
        if let (false, Some((lat, lon))) = (m.name.is_empty(), m.center) {
            labels.push(MapLabel {
                lat,
                lon,
                en: m.name.clone(),
                pt: m.name.clone(),
                kind: LabelKind::City,
                rank: LOCAL_BIG_LABEL_RANK,
                big: false,
            });
        }
    }
    log_status(
        log,
        format!("✔ Municipalities: {} municipalities", munis.len()),
    );
    let _ = tx.send(Patch {
        layer: Layer::Cities,
        pairs: Vec::new(),
        lines: vec![],
        lines_big: vec![],
        labels,
        regions: vec![],
        // This patch is appended: a replacing Cities patch would also drop the
        // base populated-place labels that share this layer.
        replace: false,
        municipal: None,
    });
}
