# SolidCraft GUI MCP Server Integration Guide

> Full documentation is available in [docs/mcp-gui.md](docs/mcp-gui.md).

SolidCraft embeds a native [Model Context Protocol (MCP)](https://modelcontextprotocol.io) server directly into the graphical CAD user interface. This eliminates the friction of launching and running `solidcraft-cli mcp` in a separate terminal window, while maintaining 100% feature and configuration parity with the command line.

---

## Key Features

1. **Integrated GUI Control Center:** Located under **Preferences > AI & MCP Server** (and accessible via **Preferences > Assistant** or the status bar indicator).
2. **Terminal Parity & Configuration Freedom:**
   - **Transport Modes:** SSE / HTTP (local port), Stdio, WebSocket.
   - **Host & Port:** Fully customizable bind address (default `127.0.0.1`) and port (default `27879`).
   - **Workspace Selection:** Active project directory display with native folder picker.
   - **Toggle & Autostart:** One-click master toggle switch, plus optional autostart on application launch.
3. **Live Status & Telemetry:**
   - Color-coded live status indicator (**Green** = Running, **Red** = Stopped, **Yellow** = Error) in Preferences, Header, and Status Bar.
   - Live collapsible telemetry log viewer displaying timestamped JSON-RPC requests, client connections, and tool results without needing a terminal.
4. **Interactive Connection Guide & 1-Click Copy:**
   - Dynamic SSE endpoint URL generation (`http://<host>:<port>/sse`) with 1-click clipboard copy.
   - Pre-formatted `mcpServers` JSON configuration snippet with 1-click clipboard copy for Google Antigravity, LM Studio, Claude Desktop, Cursor, and custom local agent frameworks.

---

## Quick Setup

1. Open SolidCraft.
2. Click the **MCP** status badge in the bottom status bar (or go to **Preferences > AI & MCP Server**).
3. Click **"Start MCP Server"**.
4. Click **"Copy Endpoint"** or **"Copy JSON"** to configure your external AI client.

For detailed architecture, protocol specifications, and troubleshooting, see [docs/mcp-gui.md](docs/mcp-gui.md).
