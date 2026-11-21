// A tiny modelling kit for the landmark miniatures.
//
// Every landmark is authored as code: primitives, lathes, extrusions and
// parametric surfaces are pushed through a transform stack and baked into ONE
// non-indexed BufferGeometry per landmark (one draw call). Colour is not a
// material: each vertex carries two albedos (night / day theme) plus an `fx`
// triple, so a single shared material can light, cross-fade and glow all nine
// models. See material.ts for the shader side.
//
//   position  vec3
//   normal    vec3   flat by default, smooth on request
//   color     vec3   albedo for the dark theme (linear)
//   colorDay  vec3   albedo for the light theme (linear)
//   fx        vec3   x: glow  (>0 warm theme emissive, <0 glows in its own colour)
//                    y: gloss (0 matte … 1 water / gilding)
//                    z: phase (>0 makes the glow twinkle, offset by this value)
import * as THREE from 'three'
import type { Tok } from './palette'
import { PALETTES } from './palette'

export type Vec = THREE.Vector3
type Num = number | ((p: Vec, n: Vec) => number)

export type Paint = {
  /** palette token */
  c: Tok
  /** second token to blend toward by `t` */
  c2?: Tok
  t?: Num
  /** further blends applied in order after c2: [token, amount] */
  m?: [Tok, number][]
  /** brightness multiplier (fake AO, streaks, weathering) */
  k?: Num
  glow?: number
  gloss?: number
  phase?: number
  /** keep the geometry's own smooth normals instead of faceting */
  smooth?: boolean
  /** evaluate colour once per triangle (crisp facets) instead of per vertex */
  face?: boolean
}
export type PaintLike = Paint | Tok | ((p: Vec, n: Vec) => Paint)

export type Xf = {
  p?: [number, number, number]
  /** euler XYZ, radians */
  r?: [number, number, number]
  s?: number | [number, number, number]
}

/* ---------------- noise ---------------- */
export function hash2(x: number, y: number) {
  const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453
  return s - Math.floor(s)
}
export function vnoise(x: number, y: number) {
  const xi = Math.floor(x), yi = Math.floor(y)
  const xf = x - xi, yf = y - yi
  const u = xf * xf * (3 - 2 * xf), v = yf * yf * (3 - 2 * yf)
  const a = hash2(xi, yi), b = hash2(xi + 1, yi), c = hash2(xi, yi + 1), d = hash2(xi + 1, yi + 1)
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}
export function fbm(x: number, y: number, oct = 3) {
  let a = 0.5, f = 1, s = 0, n = 0
  for (let i = 0; i < oct; i++) {
    s += a * vnoise(x * f, y * f)
    n += a
    a *= 0.5
    f *= 2.03
  }
  return s / n
}
/** deterministic pseudo-random stream */
export function rng(seed: number) {
  let s = seed >>> 0
  return () => {
    s = (s + 0x6d2b79f5) >>> 0
    let t = Math.imul(s ^ (s >>> 15), 1 | s)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
export const clamp = (x: number, a = 0, b = 1) => Math.min(b, Math.max(a, x))
export const lerp = (a: number, b: number, t: number) => a + (b - a) * t
export const smooth = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a))
  return t * t * (3 - 2 * t)
}
/** piecewise-linear lookup through [x, y] knots (x ascending) */
export function curve(knots: [number, number][], x: number) {
  if (x <= knots[0][0]) return knots[0][1]
  for (let i = 1; i < knots.length; i++) {
    if (x <= knots[i][0]) {
      const [x0, y0] = knots[i - 1], [x1, y1] = knots[i]
      return y0 + ((y1 - y0) * (x - x0)) / (x1 - x0)
    }
  }
  return knots[knots.length - 1][1]
}

/* ---------------- builder ---------------- */
const _a = new THREE.Vector3(), _b = new THREE.Vector3(), _c = new THREE.Vector3()
const _na = new THREE.Vector3(), _nb = new THREE.Vector3(), _nc = new THREE.Vector3()
const _fn = new THREE.Vector3(), _e1 = new THREE.Vector3(), _e2 = new THREE.Vector3(), _mid = new THREE.Vector3()
const _nm = new THREE.Matrix3()
const _e = new THREE.Euler(), _q = new THREE.Quaternion(), _s = new THREE.Vector3(), _p = new THREE.Vector3()
const _colA = new THREE.Color(), _colB = new THREE.Color()

function compose(t: Xf, out = new THREE.Matrix4()) {
  _p.set(...(t.p ?? [0, 0, 0]))
  _q.setFromEuler(_e.set(...(t.r ?? [0, 0, 0])))
  if (typeof t.s === 'number') _s.setScalar(t.s)
  else _s.set(...(t.s ?? [1, 1, 1]))
  return out.compose(_p, _q, _s)
}

const asPaint = (p: PaintLike): Paint | ((p: Vec, n: Vec) => Paint) => (typeof p === 'string' ? { c: p } : p)

export class Kit {
  private pos: number[] = []
  private nor: number[] = []
  private colA: number[] = []
  private colB: number[] = []
  private fx: number[] = []
  private drp: number[] = []
  private stack: THREE.Matrix4[] = [new THREE.Matrix4()]
  /**
   * How much the vertices added from here on follow the real terrain when the
   * model is placed (0 rigid … 1 sits on the ground). A function gets the
   * model-space position, so a mountain can be rigid at its summit and melt into
   * the land at its foot. The scene applies it; the review harness has no terrain.
   */
  drape: number | ((p: Vec) => number) = 0
  /** when set, vertices sample the terrain here (model x, z) instead of under themselves: see `rigidAt` */
  private anchor: [number, number] | null = null

  get m() {
    return this.stack[this.stack.length - 1]
  }
  get triangles() {
    return this.pos.length / 9
  }

  /** run `fn` inside a child transform */
  at(t: Xf, fn: () => void) {
    this.stack.push(this.m.clone().multiply(compose(t)))
    fn()
    this.stack.pop()
    return this
  }

  private vert(paint: Paint, p: Vec, n: Vec, cp: Vec, cn: Vec) {
    const t = typeof paint.t === 'function' ? paint.t(cp, cn) : (paint.t ?? 0)
    const k = typeof paint.k === 'function' ? paint.k(cp, cn) : (paint.k ?? 1)
    _colA.copy(PALETTES.dark[paint.c])
    _colB.copy(PALETTES.light[paint.c])
    if (paint.c2 && t > 0) {
      _colA.lerp(PALETTES.dark[paint.c2], clamp(t))
      _colB.lerp(PALETTES.light[paint.c2], clamp(t))
    }
    if (paint.m)
      for (const [tok, amt] of paint.m) {
        if (amt <= 0) continue
        _colA.lerp(PALETTES.dark[tok], clamp(amt))
        _colB.lerp(PALETTES.light[tok], clamp(amt))
      }
    this.pos.push(p.x, p.y, p.z)
    this.nor.push(n.x, n.y, n.z)
    this.colA.push(_colA.r * k, _colA.g * k, _colA.b * k)
    this.colB.push(_colB.r * k, _colB.g * k, _colB.b * k)
    this.fx.push(paint.glow ?? 0, paint.gloss ?? 0, paint.phase ?? 0)
    this.drp.push(typeof this.drape === 'function' ? this.drape(p) : this.drape, this.anchor ? this.anchor[0] : p.x, this.anchor ? this.anchor[1] : p.z, this.anchor ? 1 : 0)
  }
  /**
   * Add a free-standing object (a tree, a hut) that should ride the terrain as one
   * piece: every vertex takes the ground height, and the drape weight, found at the
   * object's foot (local x, z in the current frame) rather than under itself.
   */
  rigidAt(x: number, z: number, fn: () => void) {
    const prevAnchor = this.anchor, prevDrape = this.drape
    _p.set(x, 0, z).applyMatrix4(this.m)
    this.anchor = [_p.x, _p.z]
    if (typeof prevDrape === 'function') this.drape = prevDrape(_p)
    fn()
    this.anchor = prevAnchor
    this.drape = prevDrape
    return this
  }
  /** run `fn` with a drape weight, then restore the previous one */
  draped(w: number | ((p: Vec) => number), fn: () => void) {
    const prev = this.drape
    this.drape = w
    fn()
    this.drape = prev
    return this
  }

  /** bake a geometry (consumed) under the current transform */
  add(geo: THREE.BufferGeometry, paintLike: PaintLike, xf?: Xf) {
    const m = xf ? this.m.clone().multiply(compose(xf)) : this.m
    const g = geo.index ? geo.toNonIndexed() : geo
    const P = g.attributes.position
    const N = g.attributes.normal
    const flip = m.determinant() < 0
    _nm.getNormalMatrix(m)
    const pl = asPaint(paintLike)
    for (let i = 0; i < P.count; i += 3) {
      const i1 = flip ? i + 2 : i + 1, i2 = flip ? i + 1 : i + 2
      _a.fromBufferAttribute(P, i).applyMatrix4(m)
      _b.fromBufferAttribute(P, i1).applyMatrix4(m)
      _c.fromBufferAttribute(P, i2).applyMatrix4(m)
      _fn.crossVectors(_e1.subVectors(_b, _a), _e2.subVectors(_c, _a))
      if (_fn.lengthSq() < 1e-16) continue
      _fn.normalize()
      _mid.copy(_a).add(_b).add(_c).multiplyScalar(1 / 3)
      const facePaint = typeof pl === 'function' ? pl(_mid, _fn) : pl
      const sm = !!facePaint.smooth && !!N
      if (sm) {
        _na.fromBufferAttribute(N, i).applyMatrix3(_nm).normalize()
        _nb.fromBufferAttribute(N, i1).applyMatrix3(_nm).normalize()
        _nc.fromBufferAttribute(N, i2).applyMatrix3(_nm).normalize()
      }
      const perFace = !!facePaint.face
      const pa = perFace || typeof pl !== 'function' ? facePaint : pl(_a, sm ? _na : _fn)
      this.vert(pa, _a, sm ? _na : _fn, perFace ? _mid : _a, perFace ? _fn : sm ? _na : _fn)
      const pb = perFace || typeof pl !== 'function' ? facePaint : pl(_b, sm ? _nb : _fn)
      this.vert(pb, _b, sm ? _nb : _fn, perFace ? _mid : _b, perFace ? _fn : sm ? _nb : _fn)
      const pc = perFace || typeof pl !== 'function' ? facePaint : pl(_c, sm ? _nc : _fn)
      this.vert(pc, _c, sm ? _nc : _fn, perFace ? _mid : _c, perFace ? _fn : sm ? _nc : _fn)
    }
    if (g !== geo) g.dispose()
    geo.dispose()
    return this
  }

  /* ---- primitives: all sit on y = 0 of their local frame ---- */
  box(w: number, h: number, d: number, paint: PaintLike, xf?: Xf) {
    return this.add(new THREE.BoxGeometry(w, h, d).translate(0, h / 2, 0), paint, xf)
  }
  /** frustum / prism; `seg` sides */
  cyl(rTop: number, rBot: number, h: number, seg: number, paint: PaintLike, xf?: Xf, open = false) {
    return this.add(new THREE.CylinderGeometry(rTop, rBot, h, seg, 1, open).translate(0, h / 2, 0), paint, xf)
  }
  cone(r: number, h: number, seg: number, paint: PaintLike, xf?: Xf) {
    return this.add(new THREE.ConeGeometry(r, h, seg).translate(0, h / 2, 0), paint, xf)
  }
  /** surface of revolution from [radius, y] knots */
  lathe(pts: [number, number][], seg: number, paint: PaintLike, xf?: Xf, phiStart = 0, phiLen = Math.PI * 2) {
    return this.add(new THREE.LatheGeometry(pts.map(([r, y]) => new THREE.Vector2(Math.max(r, 1e-5), y)), seg, phiStart, phiLen), paint, xf)
  }
  ball(r: number, detail: number, paint: PaintLike, xf?: Xf) {
    return this.add(new THREE.IcosahedronGeometry(r, detail), paint, xf)
  }
  /** vertical prism over a closed [x, z] outline, from y0 up to y1 */
  prism(outline: [number, number][], y0: number, y1: number, paint: PaintLike, xf?: Xf, holes?: [number, number][][]) {
    const s = new THREE.Shape(outline.map(([x, z]) => new THREE.Vector2(x, -z)))
    if (holes) for (const h of holes) s.holes.push(new THREE.Path(h.map(([x, z]) => new THREE.Vector2(x, -z))))
    const g = new THREE.ExtrudeGeometry(s, { depth: y1 - y0, bevelEnabled: false, curveSegments: 1 })
    g.rotateX(-Math.PI / 2).translate(0, y0, 0)
    return this.add(g, paint, xf)
  }
  /** extrude an XY shape along z, centred on z = 0 */
  slab(shape: THREE.Shape, depth: number, paint: PaintLike, xf?: Xf, curveSegments = 10) {
    return this.add(new THREE.ExtrudeGeometry(shape, { depth, bevelEnabled: false, curveSegments }).translate(0, 0, -depth / 2), paint, xf)
  }
  tri(a: Vec, b: Vec, c: Vec, paint: PaintLike) {
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute([a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z], 3))
    return this.add(g, paint)
  }
  /** flat quad a→b→c→d (counter-clockwise seen from its front) */
  quad(a: [number, number, number], b: [number, number, number], c: [number, number, number], d: [number, number, number], paint: PaintLike) {
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute([...a, ...b, ...c, ...a, ...c, ...d], 3))
    return this.add(g, paint)
  }
  /**
   * Horizontal rectangle at height y, facing up. While draping it is cut into a
   * grid (`cell` wide), so stacked plates follow the same ground and never fold
   * through one another on rolling terrain.
   */
  plate(x0: number, z0: number, x1: number, z1: number, y: number, paint: PaintLike, cell = this.drape === 0 ? Infinity : 0.011) {
    const nx = Math.max(1, Math.min(40, Math.ceil((x1 - x0) / cell))), nz = Math.max(1, Math.min(40, Math.ceil((z1 - z0) / cell)))
    for (let i = 0; i < nx; i++)
      for (let j = 0; j < nz; j++) {
        const xa = x0 + ((x1 - x0) * i) / nx, xb = x0 + ((x1 - x0) * (i + 1)) / nx
        const za = z0 + ((z1 - z0) * j) / nz, zb = z0 + ((z1 - z0) * (j + 1)) / nz
        this.quad([xa, y, zb], [xb, y, zb], [xb, y, za], [xa, y, za], paint)
      }
    return this
  }
  /**
   * Parametric surface on a (nu × nv) grid. `f(u, v, out)` fills a point for u, v ∈ [0, 1].
   * closeU stitches u = 1 back to u = 0. Wind so that ∂u × ∂v points outward.
   */
  surface(nu: number, nv: number, f: (u: number, v: number, out: Vec) => void, paint: PaintLike, opts: { closeU?: boolean; xf?: Xf } = {}) {
    const cols = opts.closeU ? nu : nu + 1
    const pts = new Float32Array(cols * (nv + 1) * 3)
    const tmp = new THREE.Vector3()
    for (let j = 0; j <= nv; j++)
      for (let i = 0; i < cols; i++) {
        f(i / nu, j / nv, tmp)
        pts.set([tmp.x, tmp.y, tmp.z], (j * cols + i) * 3)
      }
    const idx: number[] = []
    for (let j = 0; j < nv; j++)
      for (let i = 0; i < nu; i++) {
        const i1 = opts.closeU ? (i + 1) % nu : i + 1
        const a = j * cols + i, b = j * cols + i1, c = (j + 1) * cols + i1, d = (j + 1) * cols + i
        // alternate the diagonal so facets do not all lean one way
        if ((i + j) % 2) idx.push(a, b, c, a, c, d)
        else idx.push(a, b, d, b, c, d)
      }
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.BufferAttribute(pts, 3))
    g.setIndex(idx)
    g.computeVertexNormals()
    return this.add(g, paint, opts.xf)
  }
  /** triangle fan closing a ring of points around a centre (cap) */
  fan(center: [number, number, number], ring: [number, number, number][], paint: PaintLike, up = true) {
    const arr: number[] = []
    for (let i = 0; i < ring.length; i++) {
      const a = ring[i], b = ring[(i + 1) % ring.length]
      if (up) arr.push(...center, ...b, ...a)
      else arr.push(...center, ...a, ...b)
    }
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute(arr, 3))
    return this.add(g, paint)
  }

  build() {
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute(this.pos, 3))
    g.setAttribute('normal', new THREE.Float32BufferAttribute(this.nor, 3))
    g.setAttribute('color', new THREE.Float32BufferAttribute(this.colA, 3))
    g.setAttribute('colorDay', new THREE.Float32BufferAttribute(this.colB, 3))
    g.setAttribute('fx', new THREE.Float32BufferAttribute(this.fx, 3))
    // drape: (weight, anchor x, anchor z, 1 if it belongs to a free-standing object). Only kept when something drapes.
    let any = false
    for (let i = 0; i < this.drp.length; i += 4) if (this.drp[i] > 0) { any = true; break }
    if (any) g.setAttribute('drape', new THREE.Float32BufferAttribute(this.drp, 4))
    g.computeBoundingBox()
    g.computeBoundingSphere()
    return g
  }
}

/* ---------------- shared set-dressing ---------------- */

/**
 * The ground a model stands on: a polar grid, so it can follow the real terrain.
 * `radius(th)` gives the outline. With `level`, the inner part (that fraction of
 * the radius) stays a rigid, level site and only the margin eases down onto the
 * land: use it with `seat: 'high'` for architecture that cannot sit on a slope.
 */
export function apron(k: Kit, radius: (th: number) => number, paint: PaintLike, opts: { y?: number; rings?: number; seg?: number; cx?: number; cz?: number; level?: number } = {}) {
  const { y = 0.0015, rings = 5, seg = 32, cx = 0, cz = 0, level } = opts
  k.draped(level === undefined ? 1 : siteWeight(radius, level, cx, cz), () => {
    k.surface(
      seg, rings,
      (u, v, out) => {
        const th = -u * Math.PI * 2
        const r = radius(th) * (1 - v)
        // the outermost ring dips under the land, so the apron has no visible edge: it just runs out
        out.set(cx + Math.cos(th) * r, v === 0 ? y - 0.006 : y, cz + Math.sin(th) * r)
      },
      paint,
      { closeU: true },
    )
  })
}
/** drape weight for everything standing on a levelled site: 0 inside `level` × radius, 1 at the rim */
export const siteWeight = (radius: (th: number) => number, level: number, cx = 0, cz = 0) => (p: Vec) => {
  const dx = p.x - cx, dz = p.z - cz
  return smooth(level, 0.98, Math.hypot(dx, dz) / radius(Math.atan2(dz, dx)))
}
/** an organic outline: an ellipse (rx, rz) with a slow wobble */
export const blob = (rx: number, rz: number, wobble = 0.12, seed = 0) => (th: number) => {
  const c = Math.cos(th), s = Math.sin(th)
  const r = (rx * rz) / Math.hypot(rz * c, rx * s)
  return r * (1 + (vnoise(c * 1.6 + 4 + seed, s * 1.6 + seed * 0.7) - 0.5) * 2 * wobble)
}

/** a faceted canopy blob */
export function tree(k: Kit, x: number, z: number, r: number, h: number, paint: PaintLike = 'foliage', seed = 0, y = 0) {
  const j = hash2(x * 91.7 + seed, z * 53.3)
  k.rigidAt(x, z, () => k.add(new THREE.IcosahedronGeometry(1, 0), paint, { p: [x, y + h * 0.55, z], r: [j * 3, j * 7, 0], s: [r, h * 0.62, r] }))
}

/** a coconut palm: leaning trunk and a crown of drooping blades */
export function palm(k: Kit, x: number, z: number, h: number, lean = 0.2, yaw = 0, trunk: PaintLike = 'palmTrunk', leaf: PaintLike = 'palmLeaf', y = 0) {
  k.rigidAt(x, z, () => k.at({ p: [x, y, z], r: [0, yaw, 0] }, () => {
    const segs = 4
    let px = 0, py = 0
    for (let i = 0; i < segs; i++) {
      const t0 = i / segs, t1 = (i + 1) / segs
      const nx = lean * h * t1 * t1, ny = h * t1
      const len = Math.hypot(nx - px, ny - py)
      const ang = Math.atan2(nx - px, ny - py)
      const r0 = h * 0.03 * (1 - t0 * 0.45), r1 = h * 0.03 * (1 - t1 * 0.45)
      k.cyl(r1, r0, len, 5, trunk, { p: [px, py, 0], r: [0, 0, -ang] })
      px = nx
      py = ny
    }
    const blades = 7
    for (let i = 0; i < blades; i++) {
      const a = (i / blades) * Math.PI * 2 + 0.3
      const L = h * 0.42, w = h * 0.075
      k.at({ p: [px, py, 0], r: [0, a, 0] }, () => {
        // two-segment blade: rises a little, then droops
        k.quad([0, 0, -w * 0.4], [L * 0.5, L * 0.16, -w], [L * 0.5, L * 0.16, w], [0, 0, w * 0.4], leaf)
        k.quad([L * 0.5, L * 0.16, -w], [L, -L * 0.12, 0], [L, -L * 0.12, 0], [L * 0.5, L * 0.16, w], leaf)
        k.quad([0, 0, w * 0.4], [L * 0.5, L * 0.16, w], [L * 0.5, L * 0.16, -w], [0, 0, -w * 0.4], leaf)
        k.quad([L * 0.5, L * 0.16, w], [L, -L * 0.12, 0], [L, -L * 0.12, 0], [L * 0.5, L * 0.16, -w], leaf)
      })
    }
  }))
}

/** hipped roof over a w × d rectangle: eaves at y = 0, ridge of length `ridge` along x at y = h */
export function hipRoof(k: Kit, w: number, d: number, h: number, ridge: number, paint: PaintLike, xf?: Xf) {
  const hw = w / 2, hd = d / 2, hr = ridge / 2
  const g = new THREE.BufferGeometry()
  // prettier-ignore
  g.setAttribute('position', new THREE.Float32BufferAttribute([
    -hw, 0, hd,  hw, 0, hd,  hr, h, 0,   -hw, 0, hd,  hr, h, 0,  -hr, h, 0, // front
     hw, 0, -hd, -hw, 0, -hd, -hr, h, 0,   hw, 0, -hd, -hr, h, 0,  hr, h, 0, // back
     hw, 0, hd,  hw, 0, -hd,  hr, h, 0, // right
    -hw, 0, -hd, -hw, 0, hd, -hr, h, 0, // left
    -hw, 0, -hd,  hw, 0, -hd,  hw, 0, hd,  -hw, 0, -hd,  hw, 0, hd,  -hw, 0, hd, // underside
  ], 3))
  return k.add(g, paint, xf)
}

/** Kandyan two-pitch roof: a broad shallow skirt with a steeper hip above it */
export function kandyanRoof(k: Kit, w: number, d: number, h: number, paint: PaintLike, xf?: Xf, ridge = Math.max(0, w - d) * 0.9) {
  k.at(xf ?? {}, () => {
    const inset = 0.7
    // skirt: frustum from the eaves up to the inner rectangle
    const hw = w / 2, hd = d / 2, iw = hw * inset, id = hd * inset, y1 = h * 0.3
    const g = new THREE.BufferGeometry()
    // prettier-ignore
    g.setAttribute('position', new THREE.Float32BufferAttribute([
      -hw, 0, hd,  hw, 0, hd,  iw, y1, id,   -hw, 0, hd,  iw, y1, id,  -iw, y1, id,
       hw, 0, -hd, -hw, 0, -hd, -iw, y1, -id,   hw, 0, -hd, -iw, y1, -id,  iw, y1, -id,
       hw, 0, hd,  hw, 0, -hd,  iw, y1, -id,   hw, 0, hd,  iw, y1, -id,  iw, y1, id,
      -hw, 0, -hd, -hw, 0, hd, -iw, y1, id,   -hw, 0, -hd, -iw, y1, id,  -iw, y1, -id,
      -hw, 0, -hd,  hw, 0, -hd,  hw, 0, hd,  -hw, 0, -hd,  hw, 0, hd,  -hw, 0, hd,
    ], 3))
    k.add(g, paint)
    hipRoof(k, iw * 2, id * 2, h * 0.7, Math.min(ridge, iw * 2 * 0.8), paint, { p: [0, y1, 0] })
  })
}
