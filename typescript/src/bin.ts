#!/usr/bin/env bun
// solidcraft-ts build <part.tsx> -o part.sc [--param name=value]... [--script replay.json]
import { resolve } from "node:path";
import { build } from "./build.ts";
import type { Module } from "./params.ts";

const USAGE = "usage: solidcraft-ts build <part.tsx> -o <part.sc> [--param NAME=VALUE]... [--script replay.json] [--no-verify]";

async function main(argv: string[]): Promise<number> {
  if (argv[0] !== "build" || !argv[1]) {
    console.error(USAGE);
    return 2;
  }
  const file = resolve(argv[1]);
  let out = "";
  let script = "";
  let verify = true;
  const params: Record<string, unknown> = {};
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === "-o" || a === "--out") out = argv[++i] ?? "";
    else if (a === "--script") script = argv[++i] ?? "";
    else if (a === "--no-verify") verify = false;
    else if (a === "--param") {
      const [k, v] = (argv[++i] ?? "").split("=", 2);
      if (!k || v === undefined) throw new Error(`--param takes NAME=VALUE`);
      params[k] = v === "true" ? true : v === "false" ? false : Number.isFinite(Number(v)) ? Number(v) : v;
    } else {
      console.error(`unknown argument ${a}\n${USAGE}`);
      return 2;
    }
  }
  if (!out) {
    console.error(USAGE);
    return 2;
  }
  const mod = (await import(file)) as Module;
  const r = await build(mod, { out, params, verify });
  if (script) await Bun.write(script, JSON.stringify(r.script, null, 1) + "\n");
  for (const b of r.bodies) console.log(`${b.name}: ${b.volume.toFixed(3)} mm³, ${b.faces} faces`);
  for (const line of r.report.expanded) console.log(`expanded: ${line}`);
  for (const line of r.report.fixed) console.log(`kept as a value: ${line}`);
  for (const p of r.report.structural) console.log(`param ${p} changes the shape of the tree: its value is built in`);
  for (const p of r.report.nonlinear) console.log(`not linear in a parameter, kept as a value: ${p}`);
  if (r.reopened) {
    for (const e of r.reopened.errors) console.error(`reopened: ${e}`);
    const bad = r.bodies.some((b) => {
      const o = r.reopened!.bodies.find((x) => x.name === b.name);
      return !o || Math.abs(o.volume - b.volume) > 1e-6 * Math.max(1, b.volume);
    });
    if (bad || r.reopened.errors.length) {
      console.error(`${out} does not rebuild to the same bodies`);
      return 1;
    }
    console.log(`${out}: saved, reopened and rebuilt`);
  }
  return 0;
}

try {
  process.exit(await main(process.argv.slice(2)));
} catch (e) {
  // SolidCraft's and the compiler's messages name the element and its line: no stack needed
  if (e instanceof Error && (e.name === "CompileError" || e.name === "SolidCraftError")) {
    console.error(`error: ${e.message}`);
    process.exit(1);
  }
  throw e;
}
