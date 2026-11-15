// The old address's side of moving designs out of the browser's storage for
// https://docs.solidcraft.io/app/ (browsers keep storage per address).
//
// In a frame of the exact origin NEW_ORIGIN (its move-designs.html, in tangency-labs/solidcraft-app)
// this answers that page's requests and shows nothing. Opened on its own it lists the designs with
// a button to save each as a file, for browsers that keep a frame's storage apart from the
// address's own (Safari may; it couldn't be tested). Framed by anything else it does nothing.
//
// It only ever reads: no write and no delete. It leaves the remembered disk files (IndexedDB) alone:
// a browser only lets the origin that picked a file use its handle, and the file is on the disk anyway.
//
// GitHub Pages can't send headers and frame-ancestors doesn't work in a meta tag, so anyone can
// frame this page. That doesn't reach the designs: every answer is posted with NEW_ORIGIN as its
// target, so the browser delivers it only to a parent of that origin, and a request is only heard
// from that parent. The ancestor check adds a refusal when another page sits above the parent.
"use strict";
(() => {
  const NEW_ORIGIN = "https://app.solidcraft.io";
  const PROTOCOL = 1;
  /** solidcraft_io::MAX_DESIGN_BYTES: the largest design the app opens. */
  const MAX_FILE_BYTES = 256 * 1024 * 1024;
  const MAX_FILES = 5000;
  /** The format number of a larger file isn't worth reading into memory. */
  const MAX_FORMAT_SCAN_BYTES = 64 * 1024 * 1024;
  const MAX_SETTING_CHARS = 8 * 1024 * 1024;
  /** apps/solidcraft-web/src/files.rs: the stable channel's names, newest first. */
  const LOCAL_PREFIXES = ["solidcraft.file.", "solvecraft.file."];
  const RECOVERY = ["recovery.sc", "recovery.solidcraft", "recovery.solvecraft"];
  const RECOVERY_META = "recovery.json";
  /** Everything else in this site's local storage belongs to the documentation. */
  const SETTINGS = ["solidcraft.prefs", "solvecraft.prefs", "solidcraft.webgl", "egui_memory_ron"];

  const storage = () => {
    try {
      return window.localStorage;
    } catch {
      return null; // Blocked.
    }
  };

  /** A local-storage file (files.rs `decode`): `t` and text, or `x` and hex. */
  function decodeLocal(value) {
    if (value.startsWith("t")) return new Blob([value.slice(1)]);
    const hex = value.startsWith("x") ? value.slice(1) : "";
    const bytes = new Uint8Array(Math.floor(hex.length / 2));
    for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(hex.substr(2 * i, 2), 16);
    return new Blob([bytes]);
  }

  /** Every stored file: name → its blob and when it was written (ms, 0 when unknown). */
  async function storedFiles() {
    const files = new Map();
    let root = null;
    try {
      root = await navigator.storage.getDirectory();
    } catch {
      // No private file storage: only local storage can hold designs here.
    }
    if (root) {
      // Pre-release channels keep folders of their own: only the stable app's files are in the root.
      for await (const handle of root.values()) {
        if (handle.kind !== "file" || handle.name.endsWith(".crswap")) continue;
        if (files.size >= MAX_FILES) break;
        const file = await handle.getFile().catch(() => null);
        if (file) files.set(handle.name, { blob: file, modified: file.lastModified });
      }
    }
    const local = storage();
    if (local) {
      for (const prefix of LOCAL_PREFIXES) {
        for (let i = 0; i < local.length && files.size < MAX_FILES; i++) {
          const key = local.key(i);
          if (!key || !key.startsWith(prefix)) continue;
          const name = key.slice(prefix.length);
          if (!files.has(name)) files.set(name, { blob: decodeLocal(local.getItem(key) ?? ""), modified: 0 });
        }
      }
    }
    return files;
  }

  /** The design format of a design file (`solidcraft/N`), or null when it can't be told. */
  async function formatOf(blob) {
    if (blob.size > MAX_FORMAT_SCAN_BYTES) return null;
    try {
      const head = new Uint8Array(await blob.slice(0, 512).arrayBuffer());
      const view = new DataView(head.buffer);
      let text = null;
      if (head.length >= 30 && view.getUint32(0, true) === 0x04034b50) {
        // A container: a zip whose first entry is document.json (crates/io/src/design.rs).
        const nameLength = view.getUint16(26, true);
        const name = new TextDecoder().decode(head.subarray(30, 30 + nameLength));
        const sizeInHeader = (view.getUint16(6, true) & 8) === 0;
        if (name !== "document.json" || !sizeInHeader) return null;
        const start = 30 + nameLength + view.getUint16(28, true);
        const body = blob.slice(start, start + view.getUint32(18, true));
        const method = view.getUint16(8, true);
        if (method === 0) text = await body.text();
        else if (method === 8 && typeof DecompressionStream === "function") {
          const inflated = body.stream().pipeThrough(new DecompressionStream("deflate-raw"));
          text = await new Response(inflated).text();
        }
      } else if (head[0] === 0x7b) {
        text = await blob.text();
      }
      if (text === null) return null;
      const match = /^(?:solidcraft|solvecraft)\/(\d+)$/.exec(JSON.parse(text).format ?? "");
      return match ? Number(match[1]) : null;
    } catch {
      return null;
    }
  }

  /**
   * What the app would show or recover: the autosave counts when it's the one the app would
   * offer, which takes its recovery.json too (files.rs `start`).
   */
  async function listing() {
    const files = await storedFiles();
    const meta = files.get(RECOVERY_META);
    const autosave = meta ? (RECOVERY.find((name) => files.has(name)) ?? null) : null;
    let autosaveName = "";
    if (autosave && meta.blob.size < 1024 * 1024) {
      autosaveName = await meta.blob.text().then((t) => JSON.parse(t).name ?? "").catch(() => "");
    }
    const entries = new Map();
    for (const [name, file] of files) {
      if (name === RECOVERY_META || (RECOVERY.includes(name) && name !== autosave)) continue;
      entries.set(name, { name, ...file, autosave: name === autosave, autosaveName: name === autosave ? String(autosaveName) : "" });
    }
    return entries;
  }

  async function answer(message) {
    switch (message.op) {
      case "hello":
        return {};
      case "list": {
        const out = [];
        for (const e of (await listing()).values()) {
          out.push({
            name: e.name,
            size: e.blob.size,
            modified: e.modified,
            autosave: e.autosave,
            autosaveName: e.autosaveName,
            format: await formatOf(e.blob),
          });
        }
        return { files: out };
      }
      case "read": {
        const entry = (await listing()).get(message.name);
        if (!entry) throw new Error("that design is no longer there");
        if (entry.blob.size > MAX_FILE_BYTES) throw new Error("that design is too large");
        return { bytes: await entry.blob.arrayBuffer() };
      }
      case "settings": {
        const local = storage();
        const settings = {};
        for (const key of SETTINGS) {
          const value = local?.getItem(key) ?? null;
          if (value !== null && value.length <= MAX_SETTING_CHARS) settings[key] = value;
        }
        return { settings };
      }
      default:
        throw new Error("unknown request");
    }
  }

  function serve() {
    window.addEventListener("message", (event) => {
      if (event.source !== window.parent || event.origin !== NEW_ORIGIN) return;
      const message = event.data;
      if (!message || message.solidcraftTransfer !== PROTOCOL || !Number.isSafeInteger(message.id)) return;
      answer(message).then(
        (result) => {
          const reply = { solidcraftTransfer: PROTOCOL, id: message.id, ok: true, ...result };
          window.parent.postMessage(reply, NEW_ORIGIN, result.bytes ? [result.bytes] : []);
        },
        (error) => {
          const reply = { solidcraftTransfer: PROTOCOL, id: message.id, ok: false, error: String(error?.message ?? error) };
          window.parent.postMessage(reply, NEW_ORIGIN);
        },
      );
    });
  }

  function save(name, blob) {
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 60000);
  }

  const size = (bytes) => (bytes < 1024 * 1024 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);

  async function show() {
    document.getElementById("page").hidden = false;
    const status = document.getElementById("status");
    const entries = [...(await listing()).values()].sort((a, b) => a.name.localeCompare(b.name));
    if (entries.length === 0) {
      status.textContent = "This browser keeps no designs for the old address. Designs you saved to a file on your computer are still there: open them at app.solidcraft.io with File > Open.";
      return;
    }
    status.textContent = entries.length === 1 ? "1 design found." : `${entries.length} designs found.`;
    document.getElementById("table").hidden = false;
    const rows = document.getElementById("rows");
    for (const entry of entries) {
      const shown = entry.autosave ? `${(entry.autosaveName || "Untitled").replace(/[\\/]/g, "-")} (unsaved work).sc` : entry.name;
      const row = rows.insertRow();
      row.insertCell().textContent = shown;
      const sizeCell = row.insertCell();
      sizeCell.className = "size";
      sizeCell.textContent = size(entry.blob.size);
      const button = document.createElement("button");
      button.textContent = "Save as a file";
      button.addEventListener("click", () => save(shown, entry.blob));
      row.insertCell().append(button);
    }
  }

  if (window.parent === window) {
    show().catch((error) => {
      document.getElementById("status").textContent = `This browser would not let the page read its storage: ${error?.message ?? error}. Use the old app below.`;
    });
    return;
  }
  // Framed: only directly by a top-level page of NEW_ORIGIN (browsers without ancestorOrigins rely
  // on the checks in `serve`, which are the ones that keep the designs in).
  const ancestors = window.location.ancestorOrigins;
  const direct = window.parent === window.top && (!ancestors || (ancestors.length === 1 && ancestors[0] === NEW_ORIGIN));
  if (direct) serve();
})();
