# Built-in GUI MCP Server Control Center

SolidCraft includes a built-in [Model Context Protocol (MCP)](https://modelcontextprotocol.io) server directly inside the graphical user interface. This eliminates the need to launch or manage a separate terminal process (`solidcraft-cli mcp`), while maintaining full parity with the CLI and preserving complete AI model freedom.

---

## 1. Overview & Key Capabilities

- **Zero-Terminal Workflow:** Start, stop, and monitor the SolidCraft MCP server with a single click inside the desktop application.
- **Full Configuration Freedom (Terminal Parity):**
  - **Transport Modes:** Choose between `SSE / HTTP (Local Port)` (default), `Stdio (Standard I/O)`, and `WebSocket`.
  - **Host / Bind Address:** Default `127.0.0.1`, fully editable for custom local network environments.
  - **Port Number:** Default `27879`, fully customizable to prevent port collisions.
  - **Workspace Directory:** Displays the current active design directory, with a native folder chooser to point agents to any project path.
  - **Autostart:** Option to automatically launch the background MCP server whenever SolidCraft starts.
- **Live Status & Telemetry Indicator:**
  - Visual status dot (**Green** = Running, **Red** = Stopped, **Yellow** = Error) visible in the Preferences panel, the Top Toolbar header, and the Status Bar.
  - Clicking the status bar indicator immediately opens the MCP Control Center.
  - Live collapsible telemetry log viewer displaying real-time incoming JSON-RPC calls, client connections, and tool execution status.
- **Interactive Connection Guide & 1-Click Copy:**
  - Dynamic client configuration snippet that updates automatically with your current Host and Port.
  - 1-Click "Copy SSE Endpoint" button (`http://127.0.0.1:27879/sse`).
  - 1-Click "Copy Configuration JSON" button for pasting directly into MCP clients.

---

## 2. Quick Start: Connecting External AI Agents

### Step 1: Open the Control Center
1. Launch SolidCraft.
2. Navigate to **Preferences > AI & MCP Server** (or **Preferences > Assistant**, or click the **MCP** pill in the bottom status bar).

### Step 2: Configure & Start
1. Ensure the **Transport Mode** is set to `SSE / HTTP (Local Port)`.
2. Confirm the Host (`127.0.0.1`) and Port (`27879`).
3. Click **"Start MCP Server"**. The status indicator will turn **Green ("Running on 127.0.0.1:27879")**.

### Step 3: Add to Your AI Client

#### A. Google Antigravity / LM Studio / Direct HTTP & SSE Clients
Click the **"Copy Endpoint"** button in the connection guide to copy:
```text
http://127.0.0.1:27879/sse
```
Paste this URL into your MCP client's SSE configuration field.

#### B. Claude Desktop, Cursor, or Standard `mcpServers` JSON
Click the **"Copy JSON"** button in the connection guide, which generates:

```json
{
  "mcpServers": {
    "solidcraft": {
      "url": "http://127.0.0.1:27879/sse"
    }
  }
}
```

Or for direct CLI bridge mode via stdio:
```json
{
  "mcpServers": {
    "solidcraft": {
      "command": "solidcraft-cli",
      "args": ["mcp", "--connect", "127.0.0.1:27878"]
    }
  }
}
```

---

## 3. Architecture & Lifecycle Management

### Background Threading
The GUI MCP server runs entirely on a lightweight, decoupled background worker thread (`solidcraft_mcp::service::McpServerController`). It bridges directly to the running SolidCraft CAD engine (`solidcraft_mcp::Remote`), ensuring that incoming JSON-RPC calls and streaming tool executions **never block or stutter the 60+ FPS 3D CAD viewport rendering loop**.

### Clean Shutdown & Port Reclamation
- Toggling **"Stop MCP Server"** cleanly aborts listener sockets and immediately frees the TCP port.
- Closing or quitting the SolidCraft application invokes the application lifecycle cleanup (`Drop for App`), guaranteeing background listener threads terminate without leaving orphaned processes.

### Safety & Crash Freedom
Following SolidCraft's strict architectural guidelines:
- Built with 100% safe, pure Rust (no unwrap, expect, or panics in non-test paths).
- Layering conforms strictly to SolidCraft architecture rules verified by `cargo xtask layers`.
- Compiles cleanly for both native desktop (`aarch64-apple-darwin`, `x86_64`) and WebAssembly (`wasm32-unknown-unknown`).
