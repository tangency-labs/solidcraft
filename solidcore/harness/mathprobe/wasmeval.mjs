// Runs the wasm32-unknown-unknown build of the same functions.
import fs from "node:fs";
const out = process.argv[2]; fs.mkdirSync(out, { recursive: true });
const { instance } = await WebAssembly.instantiate(fs.readFileSync(process.argv[3]));
const fns = ["sin","cos","tan","asin","acos","atan","sinh","cosh","ln","log10","cbrt","atan2","powf","powi"];
for (const n of fns) {
  const b = fs.readFileSync(`inputs/${n}.bin`);
  const v = new Float64Array(b.buffer, b.byteOffset, b.length / 8);
  const ar = ["atan2","powf","powi"].includes(n) ? 2 : 1;
  const f = instance.exports["p_" + n];
  const r = new Float64Array(v.length / ar);
  for (let i = 0; i < r.length; i++) r[i] = ar === 2 ? f(v[2*i], v[2*i+1]) : f(v[i]);
  fs.writeFileSync(`${out}/${n}.bin`, Buffer.from(r.buffer));
}
