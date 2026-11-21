//! Interface commands: panes, windows, the ribbon and the application (`ui.*`, `help.*`, `app.*`).
use crate::app::App;
use eframe::egui;
use globecraft_core::commands::InterfaceCommand;
use globecraft_data::settings::{
    appearance::ThemePreference,
    workspace::{PaneSide, PaneTab},
};
use serde_json::{Value, json};

const DOCUMENTATION_URL: &str = "https://github.com/FireflyTheReal/GlobeCraft#readme";
const ISSUES_URL: &str = "https://github.com/FireflyTheReal/GlobeCraft/issues/new/choose";

impl App {
    /// Runs a `ui.*`, `assistant.*`, `help.*` or `app.*` command.
    pub(super) fn run_interface(&mut self, command: InterfaceCommand) -> Result<Value, String> {
        let ctx = self.ctx.clone();
        Ok(match command {
            InterfaceCommand::LayersPane => {
                self.settings.workspace.toggle_right(PaneTab::Layers);
                json!({})
            }
            InterfaceCommand::PlacesPane => {
                self.settings.workspace.toggle_right(PaneTab::Places);
                json!({})
            }
            #[cfg(not(feature = "assistant"))]
            InterfaceCommand::AssistantPane(_) | InterfaceCommand::StopAssistant => {
                return Err("this build of GlobeCraft has no assistant".into());
            }
            #[cfg(feature = "assistant")]
            InterfaceCommand::AssistantPane(p) => {
                match p.ask.filter(|q| !q.is_empty()) {
                    Some(question) => {
                        self.settings.workspace.open_right(PaneTab::Assistant);
                        self.assistant.queue(question);
                    }
                    None => self.settings.workspace.toggle_right(PaneTab::Assistant),
                }
                json!({})
            }
            #[cfg(feature = "assistant")]
            InterfaceCommand::StopAssistant => {
                let running = self.assistant.busy();
                self.assistant_stop();
                json!({"stopped": running})
            }
            InterfaceCommand::MeasurePane => {
                self.settings.workspace.toggle_right(PaneTab::Measure);
                json!({})
            }
            InterfaceCommand::LeftPane => {
                self.settings.workspace.toggle_pane();
                json!({"shown": self.settings.workspace.right_pane.is_some()})
            }
            InterfaceCommand::PaneSide(p) => {
                let workspace = &mut self.settings.workspace;
                workspace.pane_side = match p.side.as_deref() {
                    Some(code) => PaneSide::from_code(code).ok_or("side must be left or right")?,
                    None => match workspace.pane_side {
                        PaneSide::Left => PaneSide::Right,
                        PaneSide::Right => PaneSide::Left,
                    },
                };
                json!({"side": workspace.pane_side.code()})
            }
            InterfaceCommand::MovePaneTab(p) => {
                let tab = PaneTab::from_code(&p.tab)
                    .filter(|tab| self.settings.workspace.right_is_open(*tab))
                    .ok_or("that tab is not open in the side pane")?;
                let workspace = &mut self.settings.workspace;
                workspace.move_tab(tab, usize::try_from(p.to).unwrap_or(usize::MAX));
                json!({"tabs": workspace.right_open.iter().map(|t| t.code()).collect::<Vec<_>>()})
            }
            InterfaceCommand::NavigationControls(p) => {
                let shown = p.toggled(!self.settings.hide_navigation_controls);
                self.settings.hide_navigation_controls = !shown;
                json!({"shown": shown})
            }
            InterfaceCommand::TileDiagnostics(p) => {
                let shown = p.toggled(self.frame.diagnostics);
                self.frame.diagnostics = shown;
                json!({"shown": shown})
            }
            InterfaceCommand::NavigatorStyle(p) => {
                let style =
                    globecraft_data::settings::navigation::NavigatorStyle::from_code(&p.style)
                        .ok_or("style must be globe or classic")?;
                self.settings.navigation.navigator = style;
                self.settings.hide_navigation_controls = false;
                json!({"style": style.code()})
            }
            InterfaceCommand::Ribbon => {
                self.settings.workspace.ribbon_minimized =
                    !self.settings.workspace.ribbon_minimized;
                self.chrome.ribbon_peek = false;
                json!({})
            }
            InterfaceCommand::FocusNext | InterfaceCommand::FocusPrevious => {
                let backwards = command == InterfaceCommand::FocusPrevious;
                self.focus_next_region(&ctx, backwards);
                json!({})
            }
            InterfaceCommand::PinRibbonTool(p) => {
                let (group, tool) = (p.group.as_str(), p.tool.as_str());
                let on = p.on.unwrap_or(!self.tool_pinned(group, tool));
                let entry = crate::app::tools::group_entry(group, tool);
                let toggled = on != crate::app::tools::pinned_by_default(group, tool);
                self.settings.workspace.set_ribbon_toggled(&entry, toggled);
                json!({"pinned": on})
            }
            InterfaceCommand::PinQuickTool(p) => {
                let tool = p.tool.as_str();
                crate::app::tools::parse_tool(tool).ok_or("tool is not a command")?;
                let present = self
                    .settings
                    .workspace
                    .quick_tools
                    .iter()
                    .any(|k| k == tool);
                let on = p.on.unwrap_or(!present);
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
            InterfaceCommand::EditShortcut(p) => {
                crate::app::tools::parse_tool(&p.tool).ok_or("tool is not a command")?;
                self.chrome.ribbon_tools.capture = Some(p.tool);
                json!({})
            }
            InterfaceCommand::SetShortcut(p) => {
                crate::app::tools::parse_tool(&p.tool).ok_or("tool is not a command")?;
                self.set_shortcut(&p.tool, p.keys.as_deref())?
            }
            InterfaceCommand::ResetRibbonTools => {
                let defaults = globecraft_data::settings::workspace::WorkspaceLayout::default();
                let w = &mut self.settings.workspace;
                w.ribbon_toggled = defaults.ribbon_toggled;
                w.quick_tools = defaults.quick_tools;
                w.shortcuts = defaults.shortcuts;
                crate::app::tools::sync_shortcuts(&w.shortcuts);
                json!({})
            }
            InterfaceCommand::Theme(p) => {
                self.settings.appearance.theme = match p.theme.as_str() {
                    "system" => ThemePreference::System,
                    "dark" => ThemePreference::Dark,
                    "light" => ThemePreference::Light,
                    _ => return Err("theme must be system, dark or light".into()),
                };
                json!({})
            }
            InterfaceCommand::Fullscreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                json!({})
            }
            InterfaceCommand::Minimize => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                json!({})
            }
            InterfaceCommand::CommandSearch => {
                self.windows.command_search =
                    Some(crate::app::ui::palette::CommandSearch::default());
                json!({})
            }
            InterfaceCommand::Settings => {
                if self.windows.settings && !ctx.embed_viewports() {
                    // Open in its own window: Settings… brings it to the front, as on a Mac.
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::from_hash_of(crate::app::ui::settings::VIEWPORT),
                        egui::ViewportCommand::Focus,
                    );
                } else {
                    self.windows.settings = !self.windows.settings;
                }
                json!({})
            }
            InterfaceCommand::Shortcuts => {
                self.windows.shortcuts = !self.windows.shortcuts;
                json!({})
            }
            InterfaceCommand::About => {
                self.windows.about = !self.windows.about;
                json!({})
            }
            InterfaceCommand::Documentation => {
                ctx.open_url(egui::OpenUrl::new_tab(DOCUMENTATION_URL));
                json!({})
            }
            InterfaceCommand::ReportIssue => {
                ctx.open_url(egui::OpenUrl::new_tab(ISSUES_URL));
                json!({})
            }
            InterfaceCommand::CopyDiagnostics => {
                ctx.copy_text(self.diagnostics_text());
                self.status = self.lang.text("diagnostics-copied-status").to_owned();
                json!({"copied": true})
            }
            InterfaceCommand::WorkOffline(p) => {
                let offline = p.toggled(self.settings.offline);
                self.settings.offline = offline;
                globecraft_data::network::set_offline(offline);
                json!({"offline": offline})
            }
            InterfaceCommand::Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                json!({})
            }
            InterfaceCommand::Timeline(p) => {
                // `on`, as the schema says (this read `visible`, which no caller sent).
                self.timeline.open = p.toggled(self.timeline.open);
                json!({"visible": self.timeline.open})
            }
        })
    }
}
