// Runs the WebAssembly kernels on the reference fixtures and prints every
// output as IEEE-754 bit patterns; tests/wasm_parity.rs recomputes the same
// cases natively and requires identical bits.
// node js/parity.mjs <module.wasm> <reference.json> > tests/fixtures/wasm_bits.json
import { readFileSync } from 'node:fs'

const [wasmPath, refPath] = process.argv.slice(2)
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {})
const x = instance.exports
const ref = JSON.parse(readFileSync(refPath, 'utf8'))
const view = new DataView(new ArrayBuffer(8))
const hex = (v) => { view.setFloat64(0, v, true); return view.getBigUint64(0, true).toString(16).padStart(16, '0') }
const f64 = () => new Float64Array(x.memory.buffer)
const put = (arr) => { const p = x.lsw_alloc(Math.max(arr.length, 1) * 8); f64().set(arr, p / 8); return p }
const run = (cap, fn) => { const o = x.lsw_alloc(cap * 8); const n = fn(o, cap); if (n > cap) throw new Error(`cap ${cap} < ${n}`); return Array.from(f64().slice(o / 8, o / 8 + n)).map(hex) }
const CAP = 1 << 16
const out = {}
const cv = ref.classify.values
for (let m = 0; m < 6; m++) out[`classify/${m}`] = run(CAP, (o, c) => x.lsw_classify(put(cv), cv.length, m, 5, o, c))
for (const c of ref.correlate) {
  out[`correlate/${c.name}`] = run(CAP, (o, cap) => x.lsw_correlate(put(c.x), put(c.y), c.x.length, o, cap))
  out[`preview/${c.name}`] = run(CAP, (o, cap) => x.lsw_preview_relate(put(c.x), put(c.y), c.x.length, 0, 5, o, cap))
  out[`bivariate/${c.name}`] = run(CAP, (o, cap) => x.lsw_bivariate(put(c.x), put(c.y), c.x.length, 0, 3, o, cap))
  out[`hexbin/${c.name}`] = run(CAP, (o, cap) => x.lsw_hexbin(put(c.x), put(c.y), c.x.length, 0.3, o, cap))
}
for (const r of ref.regress) {
  const n = r.y.length, flat = r.x.flat()
  out[`regress/${r.name}`] = run(CAP, (o, cap) => x.lsw_regress(put(r.y), put(flat), n, r.x.length, o, cap))
}
out['describe'] = run(CAP, (o, cap) => x.lsw_describe(put(ref.describe.values), ref.describe.values.length, o, cap))
const sorted = Object.fromEntries(Object.keys(out).sort().map((k) => [k, out[k]]))
console.log(JSON.stringify(sorted, null, 1))
