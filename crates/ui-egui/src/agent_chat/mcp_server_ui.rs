//! MCP Server Control Center and interactive connection guide for Preferences.
//!
//! Embeds the Model Context Protocol (MCP) server controls directly in the GUI with
//! real-time telemetry, live status badge, and dynamic configuration snippets for AI clients.

use egui::{Color32, RichText, vec2};
use solidcraft_mcp::{McpLogLevel, McpServerConfig, McpStatus, McpTransportMode};

use crate::theme::Tokens;
use crate::{SolveApp, ui_kit};

/// State for the MCP Server settings UI (e.g. log viewer collapse state, copy toasts).
#[derive(Default)]
pub struct McpServerUiState {
    pub logs_expanded: bool,
    pub copied_url_feedback: bool,
    pub copied_json_feedback: bool,
}

pub fn show_mcp_control_center(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let status = app.mcp_server.status();

    ui_kit::settings_section(ui, "ai_mcp_server", "Embedded MCP Server", |ui| {
        // Master status badge and Start/Stop toggle button
        ui.horizontal(|ui| {
            let (dot_color, status_text) = match &status {
                McpStatus::Running { host, port, .. } => {
                    (Color32::from_rgb(74, 222, 128), format!("Running & Listening on http://{host}:{port}/sse"))
                }
                McpStatus::Starting => (Color32::from_rgb(250, 204, 21), "Starting...".to_string()),
                McpStatus::Stopped => (Color32::from_rgb(248, 113, 113), "Stopped (Server is inactive)".to_string()),
                McpStatus::Error(e) => (Color32::from_rgb(239, 68, 68), format!("Error: {e}")),
            };

            // Draw status indicator circle
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 5.0, dot_color);
            ui.label(RichText::new(status_text).size(t.font_body).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if status.is_running() {
                    let btn = ui.button(RichText::new("Stop Server").color(Color32::from_rgb(248, 113, 113)));
                    if btn.clicked() {
                        app.mcp_server.stop();
                    }
                } else {
                    let btn = ui.button(RichText::new("Start Server").color(Color32::from_rgb(74, 222, 128)));
                    if btn.clicked() {
                        let host = app.preferences.mcp_host.clone();
                        let port = app.preferences.mcp_port;
                        let transport = McpTransportMode::from_str_loose(&app.preferences.mcp_transport);
                        let workspace = if app.preferences.mcp_workspace.trim().is_empty() {
                            None
                        } else {
                            Some(std::path::PathBuf::from(app.preferences.mcp_workspace.trim()))
                        };
                        let _ = app.mcp_server.start(McpServerConfig {
                            host,
                            port,
                            transport,
                            workspace_path: workspace,
                            control_port: None,
                            control_token: None,
                        });
                    }
                }
            });
        });

        ui.add_space(t.space_4);
        ui.separator();
        ui.add_space(t.space_4);

        // 1. Full Configuration Controls (Terminal Parity)
        ui.label(RichText::new("Configuration (Terminal Parity)").size(t.font_heading).strong());

        // Transport Mode Selector
        ui.horizontal(|ui| {
            ui.label("Transport Mode:");
            let current_mode = McpTransportMode::from_str_loose(&app.preferences.mcp_transport);
            for mode in McpTransportMode::ALL {
                let selected = current_mode == mode;
                if ui.selectable_label(selected, mode.display_name()).clicked() {
                    app.preferences.mcp_transport = mode.as_str().to_string();
                }
            }
        });

        // Host / Bind Address Input
        ui.horizontal(|ui| {
            ui.label("Host / Bind Address:");
            ui.add(egui::TextEdit::singleline(&mut app.preferences.mcp_host).desired_width(140.0));
            ui.label(RichText::new("(e.g. 127.0.0.1 or 0.0.0.0)").size(t.font_small).color(t.text_dim));
        });

        // Port Number Input
        ui.horizontal(|ui| {
            ui.label("Port Number:");
            ui.add(egui::DragValue::new(&mut app.preferences.mcp_port).range(1024..=65535));
            ui.label(RichText::new("(Default: 8989)").size(t.font_small).color(t.text_dim));
        });

        // Workspace Path Selector
        ui.horizontal(|ui| {
            ui.label("Workspace Path:");
            let display_path = if app.preferences.mcp_workspace.trim().is_empty() {
                if let Some(open_path) = app.session.path.as_ref() {
                    std::path::Path::new(open_path)
                        .parent()
                        .and_then(|p| p.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| "[Active Design Folder]".into())
                } else {
                    "[Active Design / Current Directory]".into()
                }
            } else {
                app.preferences.mcp_workspace.clone()
            };
            ui.monospace(display_path);

            #[cfg(not(target_arch = "wasm32"))]
            if ui.button("Browse...").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    app.preferences.mcp_workspace = folder.display().to_string();
                }
            }

            if !app.preferences.mcp_workspace.trim().is_empty() && ui.button("Reset").clicked() {
                app.preferences.mcp_workspace.clear();
            }
        });

        // Auto-start Checkbox
        ui.checkbox(&mut app.preferences.mcp_autostart, "Automatically start MCP server when SolidCraft launches");

        ui.add_space(t.space_6);
        ui.separator();
        ui.add_space(t.space_4);

        // 2. Built-in Interactive Connection Guide
        show_interactive_guide(app, ui, &t);

        ui.add_space(t.space_6);
        ui.separator();
        ui.add_space(t.space_4);

        // 3. Live Status & Telemetry Indicator (Collapsible Log Viewer)
        show_telemetry_logs(app, ui, &t);
    });
}

fn show_interactive_guide(app: &SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.label(RichText::new("How to Connect Your AI").size(t.font_heading).strong());
    ui.label(
        "Connect any MCP-compatible AI agent (Google Antigravity, LM Studio, Claude Desktop, Cursor, etc.) \
         to generate 2D sketches, 3D extrusions, fillets, and parametric assemblies directly in SolidCraft.",
    );

    ui.add_space(t.space_4);

    let host = &app.preferences.mcp_host;
    let port = app.preferences.mcp_port;
    let transport = McpTransportMode::from_str_loose(&app.preferences.mcp_transport);
    let sse_url = format!("http://{host}:{port}/sse");

    let mcp_json = if transport == McpTransportMode::Stdio {
        r#"{
  "mcpServers": {
    "solidcraft": {
      "command": "solidcraft-cli",
      "args": ["mcp"]
    }
  }
}"#
        .to_string()
    } else {
        format!(
            r#"{{
  "mcpServers": {{
    "solidcraft": {{
      "url": "{sse_url}"
    }}
  }}
}}"#
        )
    };

    // Direct SSE/HTTP Endpoint URL Box
    if transport != McpTransportMode::Stdio {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Direct SSE/HTTP Endpoint URL:").strong());
                if ui.button("Copy URL").clicked() {
                    ui.ctx().copy_text(sse_url.clone());
                }
            });
            ui.monospace(&sse_url);
        });
    }

    ui.add_space(t.space_2);

    // Full mcpServers JSON Configuration Block
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("AI Client Configuration (mcpServers):").strong());
            if ui.button("Copy JSON").clicked() {
                ui.ctx().copy_text(mcp_json.clone());
            }
        });
        ui.monospace(&mcp_json);
    });

    ui.add_space(t.space_4);
    ui.label(RichText::new("Quick Setup Instructions:").strong());
    ui.label("1. Keep the MCP server running via the toggle above (or leave Auto-start checked).");
    ui.label(
        "2. Paste the JSON configuration block into your AI client's config file (e.g. claude_desktop_config.json or Antigravity MCP settings).",
    );
    ui.label("3. Prompt your external AI agent with plain-English instructions, for example:");
    ui.indent("sample_prompts", |ui| {
        ui.label(RichText::new("• \"Create a 60x40mm plate extruded 10mm high with four M3 corner holes.\"").italics().color(t.text_dim));
        ui.label(RichText::new("• \"Inspect the active design and measure the total volume.\"").italics().color(t.text_dim));
        ui.label(RichText::new("• \"Add a 2mm fillet along the top outer edge.\"").italics().color(t.text_dim));
    });
}

fn show_telemetry_logs(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    let logs = app.mcp_server.logs();
    let count = logs.len();

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Live MCP Telemetry & Command Logs ({count})")).size(t.font_heading).strong());
        if ui.button("Clear Logs").clicked() {
            app.mcp_server.clear_logs();
        }
        if ui.button("Copy All Logs").clicked() {
            let full_text: String =
                logs.iter().map(|e| format!("[{}] [{}] {}", e.timestamp, e.level.as_str(), e.message)).collect::<Vec<_>>().join("\n");
            ui.ctx().copy_text(full_text);
        }
    });

    egui::ScrollArea::vertical().max_height(140.0).auto_shrink([false, false]).show(ui, |ui| {
        ui.group(|ui| {
            if logs.is_empty() {
                ui.label(
                    RichText::new("No MCP requests received yet. Start the server and connect an AI client to see live commands.").color(t.text_dim),
                );
            } else {
                for entry in logs.iter().rev() {
                    let color = match entry.level {
                        McpLogLevel::Info => Color32::from_rgb(148, 163, 184),
                        McpLogLevel::Request => Color32::from_rgb(96, 165, 250),
                        McpLogLevel::Response => Color32::from_rgb(74, 222, 128),
                        McpLogLevel::Error => Color32::from_rgb(248, 113, 113),
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&entry.timestamp).size(t.font_small).color(t.text_dim));
                        ui.label(RichText::new(format!("[{}]", entry.level.as_str())).size(t.font_small).color(color).strong());
                        ui.label(RichText::new(&entry.message).size(t.font_small).monospace());
                    });
                }
            }
        });
    });
}
