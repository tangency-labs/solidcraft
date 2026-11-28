// One design session on a headless SolidCraft: every engine command, typed by the bindings
// `cargo xtask typescript` generates from `solidcraft-cli commands --schemas`.
import { resolve } from "node:path";
import { locateCli } from "./cli.ts";
import { McpClient } from "./mcp.ts";
import type { CommandId, CommandParams } from "./generated/commands.ts";

export interface SessionOptions {
  /** A design (.sc) or script to start from. */
  design?: string;
  /** Folders the session may read and write besides the working directory. */
  allowPaths?: string[];
  /** The solidcraft-cli to run (default: found as `locateCli` finds it). */
  cli?: string;
}

export class Session {
  /** Every command sent, in order, as a replay script's steps. */
  readonly log: { command: string; params: unknown }[] = [];

  private constructor(private client: McpClient) {}

  static async open(opts: SessionOptions = {}): Promise<Session> {
    const cli = opts.cli ?? (await locateCli()).path;
    const args = ["--headless"];
    if (opts.design) args.push("--in", resolve(opts.design));
    for (const p of [process.cwd(), ...(opts.allowPaths ?? [])]) args.push("--allow-path", resolve(p));
    return new Session(await McpClient.start(cli, args));
  }

  get serverVersion(): string {
    return this.client.serverVersion;
  }

  /** Runs an engine command by its id and returns its result. */
  async run<C extends CommandId>(command: C, params: CommandParams[C]): Promise<any> {
    const clean = JSON.parse(JSON.stringify(params ?? {}));
    this.log.push({ command, params: clean });
    return this.client.callTool("execute", { command, params: clean });
  }

  /** Calls an MCP tool by name (`measure`, `inspect_design`, ...). */
  tool(name: string, args: object = {}): Promise<any> {
    return this.client.callTool(name, args);
  }

  close(): Promise<void> {
    return this.client.close();
  }
}
