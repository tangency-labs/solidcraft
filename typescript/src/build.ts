// Building a TSX module into a SolidCraft design: parameters first, then the compiled features,
// then a save; and checking the saved file the way a user opens it.
import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { locateCli, SolidCraftError } from "./cli.ts";
import { Compiler, type CompileReport } from "./compile.ts";
import { link, paramExpression, type Module } from "./params.ts";
import { Session } from "./session.ts";

export interface BuildOptions {
  /** Where to save the design (.sc, or .json for SolidCraft's plain JSON form). */
  out: string;
  /** Parameter values instead of the defaults. */
  params?: Record<string, unknown>;
  cli?: string;
  /** Reopen the saved file and check it rebuilds to the same bodies (default true). */
  verify?: boolean;
}

export interface BodyMeasure {
  name: string;
  volume: number;
  area: number;
  faces: number;
}

export interface BuildResult {
  out: string;
  /** The commands sent, as a replay script (`solidcraft-cli run`). */
  script: { commands: { command: string; params: unknown }[] };
  bodies: BodyMeasure[];
  /** The saved file reopened and rebuilt: its bodies and its timeline's errors. */
  reopened?: { bodies: BodyMeasure[]; errors: string[] };
  report: CompileReport & { structural: string[]; nonlinear: string[] };
}

export async function build(mod: Module, opts: BuildOptions): Promise<BuildResult> {
  const cli = opts.cli ?? (await locateCli()).path;
  const linked = link(mod, opts.params);
  const out = resolve(opts.out);
  mkdirSync(dirname(out), { recursive: true });
  const s = await Session.open({ cli, allowPaths: [resolve(out, "..")] });
  try {
    const c = new Compiler(s, mod.params ?? {});
    c.paramValues = linked.values;
    for (const [name, spec] of Object.entries(mod.params ?? {})) {
      if (spec.type !== "number" && spec.type !== "int") continue;
      const { expression, unit } = paramExpression(spec, linked.values[name] as number);
      const comment = spec.comment ?? spec.label;
      await s.run("parameters.add" as never, { name, expression, unit, ...(comment ? { comment } : {}) } as never);
    }
    await c.document(linked.nodes);
    const m = await s.run("inspect.measure" as never, {} as never);
    await s.run("file.save" as never, { path: out } as never);
    const result: BuildResult = {
      out,
      script: { commands: [...s.log] },
      bodies: measures(m),
      report: { ...c.report, structural: linked.structural, nonlinear: linked.nonlinear },
    };
    if (opts.verify !== false) result.reopened = reopen(cli, out);
    return result;
  } finally {
    await s.close();
  }
}

/** Opens a saved design with `solidcraft-cli eval`, which rebuilds its timeline, with parameter values. */
export function reopen(cli: string, file: string, params: Record<string, string> = {}): { bodies: BodyMeasure[]; errors: string[] } {
  const args = ["eval", file];
  for (const [k, v] of Object.entries(params)) args.push("--param", `${k}=${v}`);
  const r = spawnSync(cli, args, { encoding: "utf8", maxBuffer: 1 << 28 });
  if (r.status !== 0 && !r.stdout.trim().startsWith("{")) throw new SolidCraftError(`solidcraft-cli eval ${file}: ${r.stderr.trim() || `exit ${r.status}`}`);
  const d = JSON.parse(r.stdout) as { bodies: unknown[]; timeline?: { name: string; error: string | null }[] };
  const errors = (d.timeline ?? []).filter((t) => t.error).map((t) => `${t.name}: ${t.error}`);
  return { bodies: measures(d), errors };
}

function measures(m: any): BodyMeasure[] {
  return ((m?.bodies ?? []) as any[]).map((b) => ({ name: b.name, volume: b.volume_mm3, area: b.area_mm2, faces: b.faces }));
}
