//! Ordered patch preparation; the UI swaps one latest immutable dataset.
use super::{BoundaryData, LabelKind, Layer, MapLabel, Patch, dedupe_city_labels};
use std::sync::{Arc, Mutex, mpsc};

pub struct BoundarySnapshot {
    pub data: Arc<BoundaryData>,
    pub names: Arc<Vec<MapLabel>>,
}

pub struct BoundaryUpdates {
    latest: Arc<Mutex<Option<BoundarySnapshot>>>,
}

fn apply(data: &mut BoundaryData, patch: Patch) {
    let mut labels = patch.labels;
    match patch.layer {
        Layer::Countries => {
            data.countries = patch.lines;
            data.country_regions = patch.regions;
            data.countries_loaded = true;
            data.labels.retain(|label| label.kind != LabelKind::Country);
        }
        Layer::Cities => data
            .labels
            .retain(|label| label.from_tiles || label.kind != LabelKind::City),
        Layer::Places => {
            data.labels.retain(|label| !label.from_tiles);
            // Provinces of the big countries are named from farther out, like their lines.
            for label in &mut labels {
                if label.kind == LabelKind::State {
                    label.big = data.in_big_country(label.lat, label.lon);
                }
            }
        }
        Layer::OsmBorders => {
            let mut lines = patch.osm.unwrap_or_default();
            // Provinces of the big countries show from farther out
            // (`STATE_BIG_VISIBLE_MAX_ALTITUDE_KM`).
            let (big, other): (Vec<_>, Vec<_>) = std::mem::take(&mut lines.state)
                .into_iter()
                .partition(|line| {
                    let mid = line[line.len() / 2];
                    data.in_big_country(mid.0, mid.1)
                });
            lines.state = other;
            lines.state_big = big;
            // Municipal lines and village names follow the tiles' municipal zoom.
            if lines.municipal_max_km > 0.0 {
                data.municipal_detail_max_km = lines.municipal_max_km;
            }
            data.osm_borders = Arc::new(lines);
        }
    }
    data.labels.extend(labels);
    dedupe_city_labels(&mut data.labels);
}

impl BoundaryUpdates {
    pub fn new(
        patches: mpsc::Receiver<Patch>,
        repaint: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let latest = Arc::new(Mutex::new(None));
        let output = Arc::downgrade(&latest);
        std::thread::Builder::new()
            .name("globecraft-boundaries".into())
            .spawn(move || {
                let mut data = Arc::new(BoundaryData::default());
                while let Ok(patch) = patches.recv() {
                    if output.strong_count() == 0 {
                        return;
                    }
                    apply(Arc::make_mut(&mut data), patch);
                    // Drain already completed patches in order, then publish one snapshot.
                    for patch in patches.try_iter() {
                        apply(Arc::make_mut(&mut data), patch);
                    }
                    let snapshot = BoundarySnapshot {
                        data: data.clone(),
                        names: Arc::new(data.labels.clone()),
                    };
                    let Some(output) = output.upgrade() else {
                        return;
                    };
                    let previous = output.lock().unwrap().replace(snapshot);
                    drop(previous);
                    repaint();
                }
            })?;
        Ok(Self { latest })
    }

    pub fn take_latest(&self) -> Option<BoundarySnapshot> {
        self.latest.lock().unwrap().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn label(name: &str, kind: LabelKind, rank: u8, from_tiles: bool) -> MapLabel {
        MapLabel {
            lat: 10.0,
            lon: 10.0,
            en: name.into(),
            pt: name.into(),
            kind,
            rank,
            big: false,
            from_tiles,
        }
    }

    fn patch(layer: Layer, labels: Vec<MapLabel>) -> Patch {
        Patch {
            layer,
            lines: vec![Arc::new(vec![(0.0, 0.0), (1.0, 1.0)])],
            labels,
            regions: Vec::new(),
            osm: None,
        }
    }

    #[test]
    fn patch_preparation_publishes_one_snapshot_per_batch_in_order() {
        let (tx, rx) = mpsc::channel();
        let (wake, wakes) = mpsc::channel();
        let updates = BoundaryUpdates::new(rx, move || {
            let _ = wake.send(());
        })
        .unwrap();
        tx.send(patch(
            Layer::Countries,
            vec![label("First", LabelKind::Country, 0, false)],
        ))
        .unwrap();
        wakes.recv_timeout(Duration::from_secs(10)).unwrap();
        let first = updates.take_latest().unwrap();
        tx.send(patch(
            Layer::Countries,
            vec![label("Second", LabelKind::Country, 0, false)],
        ))
        .unwrap();
        wakes.recv_timeout(Duration::from_secs(10)).unwrap();
        let second = updates.take_latest().unwrap();
        assert_eq!(first.data.labels[0].en, "First");
        assert_eq!(second.data.labels.len(), 1);
        assert_eq!(second.names[0].en, "Second");
        assert!(second.data.has_countries());
    }

    #[test]
    fn tile_names_replace_each_other_and_keep_the_bundled_names() {
        let mut data = BoundaryData::default();
        apply(
            &mut data,
            patch(
                Layer::Cities,
                vec![label("Base city", LabelKind::City, 5, false)],
            ),
        );
        apply(
            &mut data,
            patch(
                Layer::Places,
                vec![
                    label("Old town", LabelKind::City, 11, true),
                    label("Old state", LabelKind::State, 0, true),
                ],
            ),
        );
        apply(
            &mut data,
            patch(
                Layer::Places,
                vec![label("New town", LabelKind::City, 11, true)],
            ),
        );
        let names: Vec<&str> = data.labels.iter().map(|l| l.en.as_str()).collect();
        assert!(names.contains(&"Base city") && names.contains(&"New town"));
        assert!(!names.contains(&"Old town") && !names.contains(&"Old state"));
    }
}
