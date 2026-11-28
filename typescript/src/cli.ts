// Finding `solidcraft-cli`, the program every session talks to, in the order the Python client
// uses (python/src/solidcraft/_mcp.py, _download.py): $SOLIDCRAFT_CLI, the one downloaded earlier
// for this package's version, one on PATH of the same minor version or newer, then a download of
// this version's own from the GitHub release, checked against its SHA256SUMS.txt. The cache folder
// is the Python client's, so the two share one download.
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, renameSync, rmSync, statSync } from "node:fs";
import { homedir, platform, arch } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

export const REPO = "tangency-labs/solidcraft";
const EXE = platform() === "win32" ? "solidcraft-cli.exe" : "solidcraft-cli";
export const HINT = "install SolidCraft from solidcraft.io or set SOLIDCRAFT_CLI to the path of solidcraft-cli";

export class SolidCraftError extends Error {
  override name = "SolidCraftError";
}

/** The SolidCraft version the generated bindings came from (`0.52.0`), or "" without them. */
export async function packageVersion(): Promise<string> {
  try {
    const m = (await import("./generated/commands.ts")) as { VERSION?: string };
    return m.VERSION && m.VERSION !== "0" ? m.VERSION : "";
  } catch {
    return "";
  }
}

export function cacheDir(version: string): string {
  const home = homedir();
  const base =
    platform() === "win32"
      ? process.env.LOCALAPPDATA || join(home, "AppData", "Local")
      : platform() === "darwin"
        ? join(home, "Library", "Caches")
        : process.env.XDG_CACHE_HOME || join(home, ".cache");
  return join(base, "solidcraft", "cli", version);
}

/** This platform's release archives that hold `solidcraft-cli`, best first (as the Python client). */
export function assetNames(version: string): string[] {
  const a = arch();
  switch (platform()) {
    case "linux": {
      const cpu = a === "x64" ? "x86_64" : a === "arm64" ? "aarch64" : "";
      return cpu ? [`solidcraft-cli-${version}-linux-${cpu}.tar.gz`, `solidcraft-${version}-linux-${cpu}.tar.gz`] : [];
    }
    case "darwin":
      return [`solidcraft-cli-${version}-macos-universal.zip`];
    case "win32": {
      const cpu = a === "x64" ? "x64" : a === "arm64" ? "arm64" : "";
      return cpu ? [`solidcraft-cli-${version}-windows-${cpu}.zip`, `solidcraft-${version}-windows-${cpu}-portable.zip`] : [];
    }
    case "freebsd":
      return [`solidcraft-cli-${version}-freebsd-x86_64.tar.gz`, `solidcraft-${version}-freebsd-x86_64.tar.gz`];
    default:
      return [];
  }
}

function minor(text: string): [number, number] | undefined {
  const m = /(\d+)\.(\d+)/.exec(text);
  return m ? [Number(m[1]), Number(m[2])] : undefined;
}

function onPath(): string | undefined {
  const sep = platform() === "win32" ? ";" : ":";
  for (const dir of (process.env.PATH ?? "").split(sep)) {
    if (!dir) continue;
    const p = join(dir, EXE);
    if (existsSync(p)) return p;
  }
  return undefined;
}

/** Checksums from a SHA256SUMS.txt as {name: hex}. */
export function checksums(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split(/\r?\n/)) {
    const [hex, name] = line.trim().split(/\s+/, 2);
    if (hex && name && hex.length === 64) out[name.replace(/^\*/, "")] = hex.toLowerCase();
  }
  return out;
}

async function download(version: string): Promise<string> {
  const names = assetNames(version);
  if (!names.length) throw new SolidCraftError(`the SolidCraft ${version} release has no solidcraft-cli for ${platform()} ${arch()}`);
  const base = (process.env.SOLIDCRAFT_DOWNLOAD_URL || `https://github.com/${REPO}/releases/download/v${version}`).replace(/\/$/, "");
  const sumsRes = await fetch(`${base}/SHA256SUMS.txt`, { headers: { "User-Agent": "solidcraft-typescript" } });
  if (!sumsRes.ok) throw new SolidCraftError(`cannot download ${base}/SHA256SUMS.txt: HTTP ${sumsRes.status}`);
  const sums = checksums(await sumsRes.text());
  const name = names.find((n) => n in sums);
  if (!name) throw new SolidCraftError(`the SolidCraft ${version} release lists none of ${names.join(", ")} in SHA256SUMS.txt`);
  const folder = cacheDir(version);
  mkdirSync(folder, { recursive: true });
  process.stderr.write(`solidcraft: downloading solidcraft-cli ${version}, once; it is kept in ${folder}\n`);
  const res = await fetch(`${base}/${name}`, { headers: { "User-Agent": "solidcraft-typescript" } });
  if (!res.ok) throw new SolidCraftError(`cannot download ${base}/${name}: HTTP ${res.status}`);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const got = createHash("sha256").update(bytes).digest("hex");
  if (got !== sums[name]) throw new SolidCraftError(`${name} does not match the release's SHA256SUMS.txt (expected ${sums[name]}, got ${got}); it was not run`);
  const tmp = mkdtempSync(join(folder, ".download."));
  try {
    const archive = join(tmp, name);
    await Bun.write(archive, bytes);
    // bsdtar (macOS, Windows 10+) and GNU tar both read .tar.gz; bsdtar reads .zip too.
    const r = spawnSync("tar", ["-xf", archive, "-C", tmp], { stdio: "inherit" });
    if (r.status !== 0) throw new SolidCraftError(`cannot unpack ${name} (tar exited with ${r.status})`);
    const found = findFile(tmp, EXE);
    if (!found) throw new SolidCraftError(`${name} holds no ${EXE}`);
    chmodSync(found, 0o755);
    renameSync(found, join(folder, EXE));
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  return join(folder, EXE);
}

function findFile(dir: string, name: string): string | undefined {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e === name) return p;
  }
  return undefined;
}

/** The `solidcraft-cli` to run and where it came from; may download it the first time. */
export async function locateCli(): Promise<{ path: string; from: string }> {
  const env = process.env.SOLIDCRAFT_CLI;
  if (env) return { path: env, from: "SOLIDCRAFT_CLI" };
  const version = await packageVersion();
  if (version && existsSync(join(cacheDir(version), EXE))) return { path: join(cacheDir(version), EXE), from: "downloaded for this version" };
  const found = onPath();
  let skipped = "";
  if (found) {
    const r = spawnSync(found, ["--version"], { encoding: "utf8", timeout: 30_000 });
    const want = minor(version);
    const got = r.status === 0 ? minor(r.stdout) : undefined;
    if (got && (!want || got[0] > want[0] || (got[0] === want[0] && got[1] >= want[1]))) return { path: found, from: "found on PATH" };
    skipped = `${found} (found on PATH) is ${r.status === 0 ? r.stdout.trim() : "not runnable"}; `;
  }
  if (!version) throw new SolidCraftError(`solidcraft-cli not found; ${skipped}${HINT}`);
  if (["1", "true", "yes"].includes((process.env.SOLIDCRAFT_NO_DOWNLOAD ?? "").trim().toLowerCase()))
    throw new SolidCraftError(`solidcraft-cli not found and SOLIDCRAFT_NO_DOWNLOAD is set; ${skipped}${HINT}`);
  return { path: await download(version), from: "downloaded for this version" };
}
