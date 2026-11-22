import type * as THREE from 'three'

/** a part that is drawn as its own mesh because it moves */
export type Mover = {
  geometry: THREE.BufferGeometry
  /** 'beam' = additive light cone, drawn only at night */
  kind?: 'solid' | 'beam'
  /** place the part for time `t` (seconds). Must not allocate. */
  update: (obj: THREE.Object3D, t: number) => void
}

export type Built = {
  /** everything static, baked into one geometry */
  body: THREE.BufferGeometry
  movers?: Mover[]
}

export type BuildCtx = {
  /** sea level in model units, relative to the model's ground (y = 0). Negative: the land plate stands proud of the sea. */
  sea: number
}

export type ModelDef = {
  id: string
  /** what to check the miniature against: shown in the review harness */
  notes: string[]
  build: (ctx: BuildCtx) => Built
  /**
   * Extra yaw (radians) on top of the hero-facing yaw. Models are authored with
   * their signature side on +z; the scene turns +z toward the camera, then adds this.
   */
  yaw?: number
  /** uniform scale applied in the scene */
  scale?: number
  /** radius of the pick ring, in model units. Default: an ellipse just outside the model's footprint. */
  ring?: number
  /** how much the model grows when it is the active landmark (default 0.1) */
  pop?: number
  /**
   * Coastal models: the model-space z of their shoreline. The scene turns +z toward
   * the nearest sea and slides the model so this line sits on the real coast.
   */
  shore?: number
  /**
   * Inland models: how far from the coast (model units) the origin has to be for the
   * site to fit on the land. The scene nudges the model inland until it does.
   */
  inland?: number
  /**
   * Coastal models: the compass bearing (degrees, 0 = north, 90 = east) that +z points down, for a headland
   * whose direction is a matter of record. Default: straight out to the nearest open water.
   */
  bearing?: number
  /** where the model is sited, [lon, lat], when that is not the landmark's own coordinate (a harbour's pin is in the bay, its rock is on the point) */
  site?: [number, number]
  /** flip the model left-to-right when placing it (a coastal model whose bay belongs on the other side) */
  mirror?: boolean
  /** review harness only: camera azimuth (degrees, in the model's frame) that matches how the hero sees a coastal model */
  view?: number
  /**
   * Where the model's ground plane (y = 0) is taken from.
   *   'center' (default)  the terrain under the landmark
   *   'rim'               the mean terrain height around its footprint: a mountain that replaces the terrain's own summit
   *   'high'              the highest ground within `core`: a rigid site on a slope is cut into the hill, never buried by it
   */
  seat?: 'center' | 'rim' | 'high'
  /** radius (model units) of the rigid part of the site, for seat: 'high' */
  core?: number
}
