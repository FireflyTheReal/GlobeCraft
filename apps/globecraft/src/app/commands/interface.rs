//! Panes, windows, settings, files and help. Handlers for the `ui.*`, `assistant.*`,
//! `settings.*`, `file.*`, `help.*`, `app.*` commands.
use super::*;

impl App {
    pub(in crate::app) fn run_interface(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match id {
            "file.screenshot" => {
                if self.imagery.image.is_none() {
                    return Err("The globe has not finished drawing yet.".into());
                }
                let quality = match str_param(p, "quality") {
                    Some(code) => ScreenshotQuality::from_code(code)
                        .ok_or("quality must be normal, plus, pro or pro_max")?,
                    None => self.settings.screenshot_quality,
                };
                self.take_screenshot(quality)?;
                json!({"folder": self.settings.screenshot_dir, "quality": quality.code()})
            }
            "file.cancelScreenshot" => {
                self.cancel_screenshot()?;
                json!({"cancelling": true})
            }
            "file.screenshotQuality" => {
                let quality = str_param(p, "quality")
                    .and_then(ScreenshotQuality::from_code)
                    .ok_or("quality must be normal, plus, pro or pro_max")?;
                self.settings.screenshot_quality = quality;
                json!({"quality": quality.code()})
            }
            "ui.explorer" => {
                self.settings.workspace.toggle_right(RightPane::Explorer);
                json!({})
            }
            "ui.pane.layers" | "ui.pane.places" => {
                let panel = match id {
                    "ui.pane.layers" => SidebarPanel::Layers,
                    _ => SidebarPanel::Places,
                };
                let layout = &mut self.settings.sidebar;
                if layout.open && layout.panel == panel {
                    layout.open = false;
                } else {
                    layout.panel = panel;
                    layout.open = true;
                }
                json!({})
            }
            "ui.pane.assistant" => {
                match str_param(p, "ask").filter(|q| !q.is_empty()) {
                    Some(question) => {
                        self.settings.workspace.open_right(RightPane::Assistant);
                        self.assistant.queue(question.to_string());
                    }
                    None => self.settings.workspace.toggle_right(RightPane::Assistant),
                }
                json!({})
            }
            "assistant.stop" => {
                let running = self.assistant.busy();
                self.assistant_stop();
                json!({"stopped": running})
            }
            "ui.pane.measure" => {
                self.settings.workspace.toggle_right(RightPane::Measure);
                json!({})
            }
            "ui.leftPane" => {
                self.settings.sidebar.open = !self.settings.sidebar.open;
                json!({})
            }
            "ui.navigationControls" => {
                let shown = toggled(p, !self.settings.hide_navigation_controls);
                self.settings.hide_navigation_controls = !shown;
                json!({"shown": shown})
            }
            "ui.tileDiagnostics" => {
                let shown = toggled(p, self.tile_diagnostics);
                self.tile_diagnostics = shown;
                json!({"shown": shown})
            }
            "ui.navigatorStyle" => {
                let style = str_param(p, "style")
                    .and_then(globecraft_data::settings::navigation::NavigatorStyle::from_code)
                    .ok_or("style must be globe or classic")?;
                self.settings.navigation.navigator = style;
                self.settings.hide_navigation_controls = false;
                json!({"style": style.code()})
            }
            "ui.ribbon" => {
                self.settings.workspace.ribbon_minimized =
                    !self.settings.workspace.ribbon_minimized;
                self.ribbon_peek = false;
                json!({})
            }
            "ui.focusNext" | "ui.focusPrevious" => {
                self.focus_next_region(&ctx, id == "ui.focusPrevious");
                json!({})
            }
            "ui.ribbon.pin" => {
                let group = str_param(p, "group").ok_or("group is required")?;
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                let on = bool_param(p, "on").unwrap_or(!self.tool_pinned(group, tool));
                let entry = super::super::tools::group_entry(group, tool);
                let toggled = on != super::super::tools::pinned_by_default(group, tool);
                self.settings.workspace.set_ribbon_toggled(&entry, toggled);
                json!({"pinned": on})
            }
            "ui.quickTools.pin" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                super::super::tools::parse_tool(tool).ok_or("tool is not a command")?;
                let present = self
                    .settings
                    .workspace
                    .quick_tools
                    .iter()
                    .any(|k| k == tool);
                let on = bool_param(p, "on").unwrap_or(!present);
                self.settings.workspace.set_quick_tool(tool, on);
                let now = self
                    .settings
                    .workspace
                    .quick_tools
                    .iter()
                    .any(|k| k == tool);
                if on && !now {
                    return Err(format!(
                        "the quick-access row holds at most {} tools",
                        globecraft_data::settings::workspace::MAX_QUICK_TOOLS
                    ));
                }
                json!({"pinned": now})
            }
            "ui.shortcut.edit" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                super::super::tools::parse_tool(tool).ok_or("tool is not a command")?;
                self.ribbon_tools.capture = Some(tool.to_string());
                json!({})
            }
            "ui.shortcut.set" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?.to_string();
                super::super::tools::parse_tool(&tool).ok_or("tool is not a command")?;
                self.set_shortcut(&tool, str_param(p, "keys"))?
            }
            "ui.ribbon.resetTools" => {
                let defaults = globecraft_data::settings::workspace::WorkspaceLayout::default();
                let w = &mut self.settings.workspace;
                w.ribbon_toggled = defaults.ribbon_toggled;
                w.quick_tools = defaults.quick_tools;
                w.shortcuts = defaults.shortcuts;
                super::super::tools::sync_shortcuts(&w.shortcuts);
                json!({})
            }
            "ui.theme" => {
                self.settings.appearance.theme = match str_param(p, "theme") {
                    Some("system") => ThemePreference::System,
                    Some("dark") => ThemePreference::Dark,
                    Some("light") => ThemePreference::Light,
                    _ => return Err("theme must be system, dark or light".into()),
                };
                json!({})
            }
            "ui.fullscreen" => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                json!({})
            }
            "ui.minimize" => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                json!({})
            }
            "ui.commandSearch" => {
                self.command_search = Some(super::super::ui::palette::CommandSearch::default());
                json!({})
            }
            "ui.settings" => {
                self.windows.settings = !self.windows.settings;
                json!({})
            }
            "settings.units" => {
                self.settings.distance_units = match str_param(p, "units") {
                    Some("system") => UnitPreference::System,
                    Some("metric") => UnitPreference::Metric,
                    Some("imperial") => UnitPreference::Imperial,
                    _ => return Err("units must be system, metric or imperial".into()),
                };
                json!({})
            }
            "settings.experience" => {
                self.settings.experience = str_param(p, "level")
                    .and_then(ExperienceLevel::from_code)
                    .ok_or("level must be basic or advanced")?;
                json!({})
            }
            "settings.coordinateFormat" => {
                let format = str_param(p, "format")
                    .and_then(CoordinateFormat::from_code)
                    .ok_or("unknown coordinate format")?;
                self.settings.coordinate_display.select_format(format);
                json!({})
            }
            "settings.language" => {
                let preference = str_param(p, "language")
                    .and_then(LanguagePreference::from_code)
                    .ok_or("language must be system, en or pt-BR")?;
                self.set_language_preference(preference);
                json!({"language": self.lang.code()})
            }
            "ui.shortcuts" => {
                self.windows.shortcuts = !self.windows.shortcuts;
                json!({})
            }
            "ui.about" => {
                self.windows.about = !self.windows.about;
                json!({})
            }
            "help.documentation" => {
                ctx.open_url(egui::OpenUrl::new_tab(DOCUMENTATION_URL));
                json!({})
            }
            "help.reportIssue" => {
                ctx.open_url(egui::OpenUrl::new_tab(ISSUES_URL));
                json!({})
            }
            "help.copyDiagnostics" => {
                ctx.copy_text(self.diagnostics_text());
                self.status = self.lang.text("diagnostics-copied-status").to_owned();
                json!({"copied": true})
            }
            "file.exportSettings" => {
                self.pick_settings_export();
                json!({})
            }
            "file.importSettings" => {
                self.pick_settings_import();
                json!({})
            }
            "app.workOffline" => {
                let offline = toggled(p, self.settings.offline);
                self.settings.offline = offline;
                globecraft_data::network::set_offline(offline);
                json!({"offline": offline})
            }
            "app.quit" => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                json!({})
            }
            "ui.here" => {
                let (lat, lon) = position(p)?;
                self.close_other_cards(super::super::here::Card::Here);
                self.selection.here = Some(super::super::here::HereCard { lat, lon });
                json!({"lat": lat, "lon": lon})
            }
            "ui.legend" => self.run_legend(p),
            "ui.timeline" => {
                self.timeline.open = bool_param(p, "visible").unwrap_or(!self.timeline.open);
                json!({"visible": self.timeline.open})
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }
}
