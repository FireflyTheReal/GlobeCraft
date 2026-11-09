use super::App;
use eframe::egui;
use globecraft_core::coordinates::parse_coordinate_query;
use globecraft_core::language::{Lang, fold_name};
use globecraft_data::search::merge_suggestions;
use globecraft_data::search::{GeoResult, Suggestion};
use globecraft_data::search::{geocode, photon_suggest};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
const SUGGESTION_DEBOUNCE_DELAY: Duration = Duration::from_millis(280);
const SUGGESTION_POLL_INTERVAL: Duration = Duration::from_millis(90);
/// An airport found by its code is shown from this high: its runways in view.
const AIRPORT_ALTITUDE_KM: f64 = 8.0;

/// The place search box: its text, the results of the last search and the
/// suggestions while typing.
pub(super) struct SearchState {
    pub(super) query: String,
    pub(super) results: Vec<GeoResult>,
    pub(super) rx: Option<mpsc::Receiver<Result<Vec<GeoResult>, globecraft_data::Error>>>,
    /// Fly to the best result when a search completes.
    pub(super) fly: bool,
    /// Focus the place search field on the next frame.
    pub(super) focus: bool,
    pub(super) suggestions: Vec<Suggestion>,
    pub(super) remote_suggestions: Vec<Suggestion>,
    pub(super) suggestions_open: bool,
    pub(super) selected: Option<usize>,
    pub(super) last_edit: Instant,
    pub(super) sent: String,
    pub(super) suggestions_rx: Option<mpsc::Receiver<super::SuggestionResponse>>,
    pub(super) local: globecraft_data::search::LocalSearch,
}

impl SearchState {
    pub(super) fn new(local: globecraft_data::search::LocalSearch) -> Self {
        Self {
            query: String::new(),
            results: vec![],
            rx: None,
            fly: true,
            focus: false,
            suggestions: vec![],
            remote_suggestions: vec![],
            suggestions_open: false,
            selected: None,
            last_edit: Instant::now(),
            sent: String::new(),
            suggestions_rx: None,
            local,
        }
    }
}

impl App {
    pub(super) fn on_query_changed(&mut self) {
        let q = self.search.query.trim().to_string();
        // Results and their status belong to the query that produced them.
        // Dropping its receiver also prevents a late response restoring old results.
        self.search.rx = None;
        self.search.results.clear();
        self.status = self.lang.text("app-ready-status").into();
        if q.is_empty() {
            self.search.suggestions_rx = None;
            self.search.sent.clear();
        }
        self.search.selected = None;
        self.search.last_edit = Instant::now();
        if q.is_empty() || !matches!(parse_coordinate_query(&q), Ok(None)) {
            self.search.suggestions_open = false;
            self.search.suggestions.clear();
            self.search.remote_suggestions.clear();
            return;
        }
        self.search.suggestions_open = true;
        let fq = fold_name(&q);
        self.search
            .remote_suggestions
            .retain(|s| fold_name(&s.label).contains(&fq));
        self.rebuild_suggestions();
    }

    /// Instant local matches (countries, states, cities already loaded) + remote results.
    pub(super) fn rebuild_suggestions(&mut self) {
        self.search.suggestions = self
            .search
            .local
            .merge(
                &self.search.query,
                self.lang,
                &self.search.remote_suggestions,
            )
            .unwrap_or_else(|| {
                merge_suggestions(
                    &self.search.query,
                    self.lang,
                    &self.boundaries.labels,
                    &self.search.remote_suggestions,
                )
            });
        // An airport code (FRA, EDDF) goes first.
        if let Some(airport) = self
            .live_airports
            .airports()
            .and_then(|airports| airports.by_code(&self.search.query).cloned())
        {
            self.search.suggestions.insert(
                0,
                globecraft_data::search::Suggestion {
                    label: super::live_layers::airport_label(&airport),
                    lat: airport.lat,
                    lon: airport.lon,
                    alt: AIRPORT_ALTITUDE_KM,
                },
            );
        }
        if self
            .search
            .selected
            .is_some_and(|i| i >= self.search.suggestions.len())
        {
            self.search.selected = None;
        }
    }

    pub(super) fn poll_suggestions(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.search.suggestions_rx
            && let Ok((q, res)) = rx.try_recv()
        {
            self.search.suggestions_rx = None;
            if let Ok(v) = res
                && q == self.search.query.trim()
            {
                self.search.remote_suggestions = v;
                self.rebuild_suggestions();
            }
            ctx.request_repaint();
        }
        let q = self.search.query.trim().to_string();
        if self.search.suggestions_open
            && q.chars().count() >= 2
            && q != self.search.sent
            && self.search.suggestions_rx.is_none()
        {
            if self.search.last_edit.elapsed() >= SUGGESTION_DEBOUNCE_DELAY {
                self.search.sent = q.clone();
                let (tx, rx) = mpsc::channel();
                self.search.suggestions_rx = Some(rx);
                let (ctx2, lang) = (ctx.clone(), self.lang.code());
                self.runtime.spawn(async move {
                    let r = photon_suggest(&q, lang).await;
                    let _ = tx.send((q, r));
                    ctx2.request_repaint();
                });
            } else {
                ctx.request_repaint_after(SUGGESTION_POLL_INTERVAL);
            }
        }
    }

    pub(super) fn choose_suggestion(&mut self, i: usize) {
        if let Some(s) = self.search.suggestions.get(i).cloned() {
            self.search.rx = None;
            self.search.query = s.label.clone();
            self.search.sent = s.label.clone();
            self.search.suggestions_open = false;
            self.search.selected = None;
            self.search.results.clear();
            self.fly_to(s.lat, s.lon, s.alt);
            self.status = self.lang.format(
                "search-navigation-status",
                &[("destination", s.label.into())],
            );
        }
    }

    pub(super) fn poll_search(&mut self) {
        let lg = self.lang;
        let Some(rx) = &self.search.rx else { return };
        if let Ok(res) = rx.try_recv() {
            self.search.rx = None;
            match res {
                Ok(v) if v.is_empty() => self.status = lg.text("search-empty-status").into(),
                Ok(v) => {
                    self.status = lg.format("search-results-status", &[("count", v.len().into())]);
                    let first = v[0].clone();
                    self.search.results = v;
                    // The results are listed at the top of Places.
                    let sidebar = &mut self.settings.sidebar;
                    if !sidebar.places_pinned {
                        sidebar.panel = globecraft_data::settings::sidebar::SidebarPanel::Places;
                    }
                    sidebar.open = true;
                    if self.search.fly {
                        self.fly_to(first.lat, first.lon, first.alt);
                    }
                }
                Err(e) => {
                    self.status =
                        Lang::En.format("search-request-error", &[("error", e.to_string().into())])
                }
            }
        }
    }

    pub(super) fn start_search(&mut self, ctx: &egui::Context) {
        let q = self.search.query.trim().to_string();
        if q.is_empty() {
            return;
        }
        // Secret easter egg commands are local-only (no network request).
        if matches!(fold_name(&q).as_str(), "terraplana" | "flatearth") {
            self.unlock_flat_earth(ctx);
            return;
        }
        // A view link pasted into the search box opens that view (roadmap 2.6).
        if q.starts_with(globecraft_core::view_link::PREFIX) {
            if let Err(error) = self.run("view.openLink", serde_json::json!({"link": q})) {
                self.status = error;
            }
            return;
        }
        match parse_coordinate_query(&q) {
            Ok(Some((lat, lon))) => {
                self.fly_to(lat, lon, 3.0);
                match self.settings.coordinate_display.format(lat, lon) {
                    Ok(position) => {
                        self.status = self.lang.format(
                            "search-navigation-status",
                            &[("destination", position.into())],
                        )
                    }
                    Err(error) => self.status = error.to_string(),
                }
                return;
            }
            Err(error) => {
                self.status = error.to_string();
                return;
            }
            Ok(None) => {}
        }
        if let Some(airport) = self
            .live_airports
            .airports()
            .and_then(|airports| airports.by_code(&q).cloned())
        {
            self.search.suggestions_open = false;
            self.fly_to(airport.lat, airport.lon, AIRPORT_ALTITUDE_KM);
            self.status = self.lang.format(
                "search-navigation-status",
                &[(
                    "destination",
                    super::live_layers::airport_label(&airport).into(),
                )],
            );
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.search.rx = Some(rx);
        self.status = self.lang.text("search-pending-status").into();
        let ctx = ctx.clone();
        let lang_code = self.lang.code();
        self.runtime.spawn(async move {
            let _ = tx.send(geocode(&q, lang_code).await);
            ctx.request_repaint();
        });
    }
}
