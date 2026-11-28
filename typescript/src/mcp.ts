// A minimal MCP client over stdio to `solidcraft-cli mcp --structured`: one JSON-RPC message per
// line, and every tool result carries its reply as data (structuredContent {result, notes?}), so
// nothing here parses text. The same protocol the Python client speaks (python/src/solidcraft/_mcp.py).
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { SolidCraftError } from "./cli.ts";

const PROTOCOL = "2025-06-18";

export class McpClient {
  private p: ChildProcessWithoutNullStreams;
  private next = 0;
  private waiting = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private chain: Promise<unknown> = Promise.resolve();
  private exited = false;
  serverVersion = "";

  private constructor(cli: string, args: string[]) {
    this.p = spawn(cli, ["mcp", "--structured", ...args], { stdio: ["pipe", "pipe", "pipe"] });
    this.p.stderr.resume();
    createInterface({ input: this.p.stdout }).on("line", (line) => {
      let msg: { id?: number; result?: unknown; error?: { message?: string } };
      try {
        msg = JSON.parse(line);
      } catch {
        return;
      }
      if (typeof msg.id !== "number") return;
      const w = this.waiting.get(msg.id);
      if (!w) return;
      this.waiting.delete(msg.id);
      if (msg.error) w.reject(new SolidCraftError(msg.error.message ?? JSON.stringify(msg.error)));
      else w.resolve(msg.result ?? {});
    });
    this.p.on("exit", () => {
      this.exited = true;
      for (const w of this.waiting.values()) w.reject(new SolidCraftError("the SolidCraft MCP server exited"));
      this.waiting.clear();
    });
  }

  static async start(cli: string, args: string[]): Promise<McpClient> {
    const c = new McpClient(cli, args);
    const init = (await c.rpc("initialize", {
      protocolVersion: PROTOCOL,
      capabilities: {},
      clientInfo: { name: "solidcraft-typescript", version: "0" },
    })) as { serverInfo?: { version?: string } };
    c.serverVersion = init.serverInfo?.version ?? "";
    c.send({ jsonrpc: "2.0", method: "notifications/initialized" });
    return c;
  }

  private send(msg: object): void {
    if (this.exited) throw new SolidCraftError("the SolidCraft MCP server exited");
    this.p.stdin.write(JSON.stringify(msg) + "\n");
  }

  rpc(method: string, params: object): Promise<unknown> {
    const run = () =>
      new Promise<unknown>((resolve, reject) => {
        const id = ++this.next;
        this.waiting.set(id, { resolve, reject });
        try {
          this.send({ jsonrpc: "2.0", id, method, params });
        } catch (e) {
          this.waiting.delete(id);
          reject(e as Error);
        }
      });
    // One request at a time, in order: a design session is a sequence of edits.
    const r = this.chain.then(run, run);
    this.chain = r.catch(() => undefined);
    return r;
  }

  /** A tool's result; throws SolidCraftError with SolidCraft's message on an error result. */
  async callTool(name: string, args: object): Promise<unknown> {
    const r = (await this.rpc("tools/call", { name, arguments: args })) as {
      structuredContent?: { result?: unknown };
      isError?: boolean;
    };
    if (!r.structuredContent && !r.isError)
      throw new SolidCraftError(`solidcraft-cli ${this.serverVersion} sends no structured results; set SOLIDCRAFT_CLI to a newer solidcraft-cli`);
    const result = r.structuredContent?.result;
    if (r.isError) throw new SolidCraftError(`${name}: ${typeof result === "string" ? result : JSON.stringify(result)}`);
    return result;
  }

  async close(): Promise<void> {
    if (this.exited) return;
    const done = new Promise<void>((resolve) => this.p.once("exit", () => resolve()));
    this.p.stdin.end();
    const timer = setTimeout(() => this.p.kill(), 10_000);
    await done;
    clearTimeout(timer);
  }
}
