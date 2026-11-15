//! Interface commands: panes, windows, the ribbon and the application (`ui.*`, `help.*`, `app.*`).
use super::{bool_param, str_param, toggled};
use crate::app::App;
use eframe::egui;
use globecraft_data::settings::{
    appearance::ThemePreference, sidebar::SidebarPanel, workspace::RightPane,
};
use serde_json::{Value, json};

const DOCUMENTATION_URL: &str = "https://github.com/FireflyTheReal/GlobeCraft#readme";
const ISSUES_URL: &str = "https://github.com/FireflyTheReal/GlobeCraft/issues/new/choose";

impl App {
    /// Runs a `ui.*`, `assistant.*`, `help.*` or `app.*` command.
    pub(super) fn run_interface(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match id {
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
            #[cfg(not(feature = "assistant"))]
            "ui.pane.assistant" | "assistant.stop" => {
                return Err("this build of GlobeCraft has no assistant".into());
            }
            #[cfg(feature = "assistant")]
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
            #[cfg(feature = "assistant")]
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
                let shown = toggled(p, self.frame.diagnostics);
                self.frame.diagnostics = shown;
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
                self.chrome.ribbon_peek = false;
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
                let entry = crate::app::tools::group_entry(group, tool);
                let toggled = on != crate::app::tools::pinned_by_default(group, tool);
                self.settings.workspace.set_ribbon_toggled(&entry, toggled);
                json!({"pinned": on})
            }
            "ui.quickTools.pin" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?;
                crate::app::tools::parse_tool(tool).ok_or("tool is not a command")?;
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
                crate::app::tools::parse_tool(tool).ok_or("tool is not a command")?;
                self.chrome.ribbon_tools.capture = Some(tool.to_string());
                json!({})
            }
            "ui.shortcut.set" => {
                let tool = str_param(p, "tool").ok_or("tool is required")?.to_string();
                crate::app::tools::parse_tool(&tool).ok_or("tool is not a command")?;
                self.set_shortcut(&tool, str_param(p, "keys"))?
            }
            "ui.ribbon.resetTools" => {
                let defaults = globecraft_data::settings::workspace::WorkspaceLayout::default();
                let w = &mut self.settings.workspace;
                w.ribbon_toggled = defaults.ribbon_toggled;
                w.quick_tools = defaults.quick_tools;
                w.shortcuts = defaults.shortcuts;
                crate::app::tools::sync_shortcuts(&w.shortcuts);
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
                self.windows.command_search =
                    Some(crate::app::ui::palette::CommandSearch::default());
                json!({})
            }
            "ui.settings" => {
                self.windows.settings = !self.windows.settings;
                json!({})
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
            "ui.timeline" => {
                self.timeline.open = bool_param(p, "visible").unwrap_or(!self.timeline.open);
                json!({"visible": self.timeline.open})
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }
}
