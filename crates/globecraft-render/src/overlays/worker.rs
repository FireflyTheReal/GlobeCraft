//! Bounded background geometry preparation; font metrics remain backend-owned.
use super::{
    BorderDashes, BorderKind, BoundaryIndex, OverlayLayers, OverlayProjection, project_borders,
    project_labels,
};
use emath::{Pos2, Rect};
use globecraft_core::camera::Basis;
use globecraft_data::boundaries::BoundaryData;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, PartialEq)]
pub struct OverlayView {
    pub basis: Basis,
    pub rect: Rect,
    pub altitude_km: f64,
    /// Matches `OverlayProjection::state_visible_max_km`.
    pub state_visible_max_km: f64,
    pub grid_step_deg: Option<f64>,
    pub layers: OverlayLayers,
    /// Matches `OverlayProjection::dashes`.
    pub dashes: BorderDashes,
}
pub struct PreparedLabel {
    pub position: Pos2,
    pub source_index: usize,
}
pub struct OverlayGeometry {
    pub view: OverlayView,
    pub data: Arc<BoundaryData>,
    pub borders: Vec<(BorderKind, [Pos2; 2])>,
    pub labels: Vec<PreparedLabel>,
    pub grid_labels: Vec<super::grid::GridLabel>,
}
impl OverlayGeometry {
    pub fn matches(&self, view: OverlayView, data: &Arc<BoundaryData>) -> bool {
        self.view == view && Arc::ptr_eq(&self.data, data)
    }
}
pub struct OverlayInput {
    pub view: OverlayView,
    pub data: Arc<BoundaryData>,
}

#[derive(Default)]
pub struct OverlayPreparer {
    indexed: Option<(Arc<BoundaryData>, BoundaryIndex)>,
    latest: Option<Arc<OverlayGeometry>>,
}
impl OverlayPreparer {
    pub fn prepare(&mut self, request: OverlayInput) -> Arc<OverlayGeometry> {
        #[cfg(feature = "profiling")]
        let _profile = globecraft_data::profiling::Span::new("overlay.prepare");
        if let Some(geometry) = self
            .latest
            .as_ref()
            .filter(|geometry| geometry.matches(request.view, &request.data))
        {
            return geometry.clone();
        }
        let index = match &mut self.indexed {
            Some((data, index)) if Arc::ptr_eq(data, &request.data) => &*index,
            slot => {
                #[cfg(feature = "profiling")]
                let _profile = globecraft_data::profiling::Span::new("overlay.index");
                let index =
                    BoundaryIndex::reusing(&request.data, slot.as_ref().map(|(_, index)| index));
                &slot.insert((request.data.clone(), index)).1
            }
        };
        let visible = index.visible(&request.view.basis, request.view.rect);
        let projection = OverlayProjection {
            basis: &request.view.basis,
            rect: request.view.rect,
            altitude_km: request.view.altitude_km,
            state_visible_max_km: request.view.state_visible_max_km,
            dashes: request.view.dashes,
            layers: request.view.layers,
        };
        let mut borders = Vec::new();
        project_borders(
            &projection,
            &request.data,
            Some(&visible),
            |kind, segment| borders.push((kind, segment)),
        );
        let labels = project_labels(&projection, &request.data)
            .into_iter()
            .map(|label| PreparedLabel {
                position: label.position,
                source_index: label.source_index,
            })
            .collect();
        let grid_labels = request.view.grid_step_deg.map_or_else(Vec::new, |step| {
            super::grid::project_grid_labels(&request.view.basis, request.view.rect, step)
        });
        let geometry = Arc::new(OverlayGeometry {
            view: request.view,
            data: request.data,
            borders,
            labels,
            grid_labels,
        });
        self.latest = Some(geometry.clone());
        geometry
    }
}

struct Mailbox<T> {
    pending: Option<OverlayInput>,
    completed: Option<T>,
    stopped: bool,
}
struct Shared<T> {
    mailbox: Mutex<Mailbox<T>>,
    wake: Condvar,
}

/// Prepares overlay geometry on its own thread, one coalesced view at a time.
/// `T` is what the frontend publishes: the geometry itself, or the geometry plus
/// backend-specific work (such as tessellated border meshes) done by the
/// `finish` step on the same thread.
pub struct OverlayWorker<T = Arc<OverlayGeometry>> {
    shared: Arc<Shared<T>>,
}

impl OverlayWorker {
    pub fn new(repaint: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        Self::with_finish(repaint, |geometry| geometry)
    }
}

impl<T: Send + 'static> OverlayWorker<T> {
    pub fn with_finish(
        repaint: impl Fn() + Send + 'static,
        mut finish: impl FnMut(Arc<OverlayGeometry>) -> T + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox {
                pending: None,
                completed: None,
                stopped: false,
            }),
            wake: Condvar::new(),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("globecraft-overlays".into())
            .spawn(move || {
                let mut preparer = OverlayPreparer::default();
                loop {
                    let request = {
                        let mut state = worker.mailbox.lock().unwrap();
                        while state.pending.is_none() && !state.stopped {
                            state = worker.wake.wait(state).unwrap();
                        }
                        if state.stopped {
                            return;
                        }
                        state.pending.take().unwrap()
                    };
                    let finished = finish(preparer.prepare(request));
                    let mut state = worker.mailbox.lock().unwrap();
                    if state.stopped {
                        return;
                    }
                    state.completed = Some(finished);
                    drop(state);
                    repaint();
                }
            })?;
        Ok(Self { shared })
    }
    pub fn submit(&self, view: OverlayView, data: Arc<BoundaryData>) {
        self.shared.mailbox.lock().unwrap().pending = Some(OverlayInput { view, data });
        self.shared.wake.notify_one();
    }
    pub fn take_completed(&self) -> Option<T> {
        self.shared.mailbox.lock().unwrap().completed.take()
    }
}
impl<T> Drop for OverlayWorker<T> {
    fn drop(&mut self) {
        let mut state = self.shared.mailbox.lock().unwrap();
        state.stopped = true;
        state.pending = None;
        state.completed = None;
        drop(state);
        self.shared.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;
    use globecraft_data::boundaries::{LabelKind, MapLabel};
    use std::{sync::mpsc, time::Duration};
    #[test]
    fn decimated_border_runs_stay_connected_at_high_altitude() {
        // A run of steps that are individually below the decimation threshold
        // must accumulate into connected chords against the last emitted
        // vertex. Advancing the anchor to every skipped vertex instead drops
        // each short step on its own and collapses fine borders into dots.
        let mut boundary_data = BoundaryData::default();
        // 80 vertices of 0.05° each: at 6,000 km the whole run spans a few
        // hundred pixels of latitude-neutral coastline, so every projected
        // step is far below the 1.4-point threshold and stays on screen.
        boundary_data.countries = vec![Arc::new(
            (0..80).map(|i| (10.0, 20.0 + i as f64 * 0.05)).collect(),
        )];
        let data = Arc::new(boundary_data);
        let view = OverlayView {
            basis: Cam {
                lat: 10.0,
                lon: 22.0,
                heading: 0.0,
                alt: 6000.0,
                tilt: 0.0,
            }
            .basis(1.0),
            rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(600.0, 600.0)),
            altitude_km: 6000.0,
            state_visible_max_km: 1609.0,
            dashes: BorderDashes::default(),
            grid_step_deg: None,
            layers: OverlayLayers {
                countries: true,
                states: false,
                cities: false,
                labels: false,
                sun: None,
            },
        };
        let projection = OverlayProjection {
            basis: &view.basis,
            rect: view.rect,
            altitude_km: view.altitude_km,
            state_visible_max_km: view.state_visible_max_km,
            dashes: view.dashes,
            layers: view.layers,
        };
        let mut segments = Vec::new();
        project_borders(&projection, &data, None, |_, points| segments.push(points));
        assert!(!segments.is_empty(), "fine border collapsed into nothing");
        for pair in segments.windows(2) {
            assert_eq!(
                pair[0][1], pair[1][0],
                "emitted chord must start where the previous one ended"
            );
        }
    }

    #[test]
    fn background_geometry_matches_direct_projection_and_dataset_identity() {
        let mut boundary_data = BoundaryData::default();
        boundary_data.countries = vec![Arc::new(vec![(0.0, 0.0), (1.0, 1.0)])];
        boundary_data.labels = vec![MapLabel {
            lat: 0.0,
            lon: 0.0,
            en: "Center".into(),
            pt: "Centro".into(),
            kind: LabelKind::Country,
            rank: 0,
            big: false,
            from_tiles: false,
        }];
        let data = Arc::new(boundary_data);
        let view = OverlayView {
            basis: Cam {
                lat: 0.0,
                lon: 0.0,
                heading: 0.0,
                alt: 1000.0,
                tilt: 0.0,
            }
            .basis(1.0),
            rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(600.0, 600.0)),
            altitude_km: 1000.0,
            state_visible_max_km: 1609.0,
            dashes: BorderDashes::default(),
            grid_step_deg: None,
            layers: OverlayLayers {
                countries: true,
                states: true,
                cities: false,
                labels: true,
                sun: None,
            },
        };
        let (tx, rx) = mpsc::channel();
        let ui_thread = std::thread::current().id();
        let worker = OverlayWorker::new(move || {
            assert_ne!(std::thread::current().id(), ui_thread);
            let _ = tx.send(());
        })
        .unwrap();
        worker.submit(view, data.clone());
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let geometry = worker.take_completed().unwrap();
        assert!(geometry.matches(view, &data));
        assert!(!geometry.matches(view, &Arc::new((*data).clone())));
        let projection = OverlayProjection {
            basis: &view.basis,
            rect: view.rect,
            altitude_km: view.altitude_km,
            state_visible_max_km: view.state_visible_max_km,
            dashes: view.dashes,
            layers: view.layers,
        };
        let mut direct = Vec::new();
        project_borders(&projection, &data, None, |_, points| direct.push(points));
        assert_eq!(
            geometry
                .borders
                .iter()
                .map(|(_, points)| *points)
                .collect::<Vec<_>>(),
            direct
        );
        let labels = project_labels(&projection, &data);
        assert_eq!(geometry.labels[0].position, labels[0].position);
        assert_eq!(geometry.labels[0].source_index, labels[0].source_index);

        let mut preparer = OverlayPreparer::default();
        let first = preparer.prepare(OverlayInput {
            view,
            data: data.clone(),
        });
        let same = preparer.prepare(OverlayInput {
            view,
            data: data.clone(),
        });
        assert!(
            Arc::ptr_eq(&first, &same),
            "tile-only changes reuse geometry"
        );
        let hidden = OverlayView {
            layers: OverlayLayers {
                labels: false,
                ..view.layers
            },
            ..view
        };
        let next = preparer.prepare(OverlayInput {
            view: hidden,
            data: data.clone(),
        });
        assert!(!Arc::ptr_eq(&first, &next));
        assert!(next.labels.is_empty());
        let numbered = OverlayView {
            grid_step_deg: Some(2.0),
            ..view
        };
        worker.submit(numbered, data.clone());
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let numbered_geometry = worker.take_completed().unwrap();
        assert!(!numbered_geometry.matches(view, &data));
        assert!(!numbered_geometry.grid_labels.is_empty());
        assert!(first.grid_labels.is_empty());
        assert!(!numbered_geometry.matches(
            OverlayView {
                grid_step_deg: Some(1.0),
                ..numbered
            },
            &data
        ));
        worker.submit(view, data.clone());
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(worker.take_completed().unwrap().grid_labels.is_empty());
    }

    #[test]
    fn pending_views_coalesce_while_completed_work_is_busy() {
        let (wake, wakes) = mpsc::channel();
        let (release, releases) = mpsc::channel();
        let worker = OverlayWorker::new(move || {
            let _ = wake.send(());
            let _ = releases.recv_timeout(Duration::from_secs(10));
        })
        .unwrap();
        let data = Arc::new(BoundaryData::default());
        let mut view = OverlayView {
            basis: Cam {
                lat: 0.0,
                lon: 0.0,
                heading: 0.0,
                alt: 1000.0,
                tilt: 0.0,
            }
            .basis(1.0),
            rect: Rect::from_min_size(Pos2::ZERO, emath::vec2(600.0, 600.0)),
            altitude_km: 1000.0,
            state_visible_max_km: 1609.0,
            dashes: BorderDashes::default(),
            grid_step_deg: None,
            layers: OverlayLayers {
                countries: true,
                states: true,
                cities: false,
                labels: true,
                sun: None,
            },
        };
        worker.submit(view, data.clone());
        wakes.recv_timeout(Duration::from_secs(10)).unwrap();
        for altitude in 1..=100 {
            view.altitude_km = altitude as f64;
            worker.submit(view, data.clone());
        }
        release.send(()).unwrap();
        wakes.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(worker.take_completed().unwrap().matches(view, &data));
        release.send(()).unwrap();
    }
}
