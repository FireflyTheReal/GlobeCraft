//! Settings → Network: every built-in service, the hosts it uses, its request
//! limit and user agent, plus the hosts that come from the user's own settings.
//! The table itself lives in `globecraft_data::network`.
use crate::app::App;
use eframe::egui;
use globecraft_core::language::{Lang, fold_name};
use globecraft_data::network::{self, Category, RequestLimit, SERVICES, Service, TileService};
use globecraft_data::user_data::UserSource;

impl App {
    pub(super) fn settings_network(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.weak(lang.text("settings-network-intro"));
        for category in Category::ALL {
            match category {
                Category::Basemap => {
                    group_title(ui, lang, category);
                    for service in network::tile_services() {
                        tile_service_row(ui, lang, &service);
                    }
                    ui.weak(lang.format(
                        "settings-network-tile-agent",
                        &[("agent", network::TILE_USER_AGENT.into())],
                    ));
                }
                Category::UserData => self.network_user_layers(ui),
                Category::Assistant => self.network_assistant(ui),
                Category::MapData | Category::Search | Category::Live | Category::Updates => {
                    group_title(ui, lang, category);
                    for service in SERVICES
                        .iter()
                        .filter(|service| service.category == category)
                    {
                        service_row(ui, lang, service);
                    }
                }
            }
        }
    }

    fn network_user_layers(&self, ui: &mut egui::Ui) {
        let lang = self.lang;
        group_title(ui, lang, Category::UserData);
        let mut listed = false;
        for layer in &self.settings.user_layers {
            let Some(host) = layer.network_host() else {
                continue;
            };
            listed = true;
            match &layer.source {
                UserSource::Features {
                    refresh_minutes, ..
                } if *refresh_minutes > 0 => {
                    ui.label(lang.format(
                        "settings-network-user-layer-reload",
                        &[
                            ("name", layer.name.as_str().into()),
                            ("host", host.into()),
                            ("minutes", (*refresh_minutes).to_string().into()),
                        ],
                    ));
                }
                _ => {
                    ui.label(lang.format(
                        "settings-network-user-layer-host",
                        &[("name", layer.name.as_str().into()), ("host", host.into())],
                    ));
                }
            }
        }
        if !listed {
            ui.weak(lang.text("settings-network-user-layer-none"));
        }
        ui.weak(lang.text("settings-network-user-layer-help"));
    }

    fn network_assistant(&self, ui: &mut egui::Ui) {
        let lang = self.lang;
        group_title(ui, lang, Category::Assistant);
        let Some((preset, base_url)) = self.assistant_target() else {
            ui.weak(lang.text("settings-network-assistant-none"));
            return;
        };
        match network::host_of_url(&base_url) {
            Some(host) => ui.label(lang.format(
                "settings-network-assistant-selected",
                &[("provider", preset.label.into()), ("host", host.into())],
            )),
            None => ui.label(lang.format(
                "settings-network-assistant-invalid",
                &[("provider", preset.label.into())],
            )),
        };
        ui.weak(lang.text("settings-network-assistant-help"));
    }

    /// The provider the assistant would use and its effective base address.
    fn assistant_target(
        &self,
    ) -> Option<(&'static globecraft_assistant::provider::Preset, String)> {
        let provider = self.configured_assistant_provider()?;
        let preset = globecraft_assistant::provider::preset(&provider)?;
        let mut base_url = globecraft_assistant::Config::from_env(&provider).base_url;
        if let Some(configured) = self
            .settings
            .assistant
            .base_urls
            .get(&provider)
            .map(String::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            base_url = configured.to_string();
        }
        Some((preset, base_url))
    }

    /// Whether a search for `query` names a host or service in the dynamic rows.
    pub(super) fn network_dynamic_matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        self.settings.user_layers.iter().any(|layer| {
            fold_name(&layer.name).contains(query)
                || layer
                    .network_host()
                    .is_some_and(|host| fold_name(host).contains(query))
        }) || self.assistant_target().is_some_and(|(preset, base_url)| {
            fold_name(preset.label).contains(query)
                || fold_name(preset.id).contains(query)
                || network::host_of_url(&base_url)
                    .is_some_and(|host| fold_name(host).contains(query))
        })
    }
}

/// Whether a search names a built-in host, service id or product name.
pub(super) fn network_matches(query: &str) -> bool {
    use globecraft_data::network as policy;
    policy::SERVICES.iter().any(|service| {
        fold_name(service.name(Lang::En)).contains(query)
            || fold_name(service.id).contains(query)
            || service
                .hosts
                .iter()
                .any(|host| fold_name(host).contains(query))
    }) || policy::tile_services().any(|service| {
        fold_name(service.name(Lang::En)).contains(query)
            || fold_name(service.provider.code()).contains(query)
            || service
                .host
                .is_some_and(|host| fold_name(host).contains(query))
    })
}

fn group_title(ui: &mut egui::Ui, lang: Lang, category: Category) {
    ui.add_space(6.0);
    ui.strong(lang.text(category.label_id()));
}

fn tile_service_row(ui: &mut egui::Ui, lang: Lang, service: &TileService) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(service.name(lang)).strong());
        if let Some(host) = service.host {
            ui.label(egui::RichText::new(host).monospace().small());
        }
    });
    ui.weak(limit_text(lang, service.limit));
    if let (Some(count), Some(host)) = (service.host_limit, service.host) {
        ui.weak(lang.format(
            "settings-network-host-shared",
            &[("count", count.to_string().into()), ("host", host.into())],
        ));
    }
    ui.weak(lang.text(if service.on_demand {
        "settings-network-tiles-on-demand"
    } else {
        "settings-network-tiles-prefetch"
    }));
}

fn service_row(ui: &mut egui::Ui, lang: Lang, service: &Service) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(service.name(lang)).strong());
        for host in service.hosts {
            ui.label(egui::RichText::new(*host).monospace().small());
        }
    });
    ui.weak(limit_text(lang, service.limit));
    match service.user_agent {
        Some(agent) => ui.weak(lang.format("settings-network-agent", &[("agent", agent.into())])),
        None => ui.weak(lang.text("settings-network-agent-updater")),
    };
}

fn limit_text(lang: Lang, limit: RequestLimit) -> String {
    match limit {
        RequestLimit::Parallel(count) => lang.format(
            "settings-network-limit-parallel",
            &[("count", count.to_string().into())],
        ),
        RequestLimit::Every(seconds) if seconds % 3600 == 0 => lang.format(
            "settings-network-limit-hours",
            &[("hours", (seconds / 3600).to_string().into())],
        ),
        RequestLimit::Every(seconds) if seconds % 60 == 0 => lang.format(
            "settings-network-limit-minutes",
            &[("minutes", (seconds / 60).to_string().into())],
        ),
        RequestLimit::Every(seconds) => lang.format(
            "settings-network-limit-seconds",
            &[("seconds", seconds.to_string().into())],
        ),
        RequestLimit::OnDemand => lang.text("settings-network-limit-on-demand").to_string(),
        RequestLimit::Once => lang.text("settings-network-limit-once").to_string(),
        RequestLimit::UserControlled => lang
            .text("settings-network-limit-user-controlled")
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_limit_reads_as_the_largest_whole_unit() {
        assert_eq!(
            limit_text(Lang::En, RequestLimit::Parallel(6)),
            "Up to 6 requests at once"
        );
        assert_eq!(
            limit_text(Lang::En, RequestLimit::Every(10)),
            "Normally one request every 10 s"
        );
        assert_eq!(
            limit_text(Lang::En, RequestLimit::Every(17 * 60)),
            "Normally one request every 17 min"
        );
        assert_eq!(
            limit_text(Lang::En, RequestLimit::Every(6 * 60 * 60)),
            "Normally one request every 6 h"
        );
        assert_eq!(
            limit_text(Lang::En, RequestLimit::OnDemand),
            "Only when needed"
        );
    }
}
