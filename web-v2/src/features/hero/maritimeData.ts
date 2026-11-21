// Mock maritime picture for the hero: ports, shipping lanes and a fleet.
// Geometry is plausible (real port positions, the Dondra Head traffic lanes roughly
// 15 nm south of the cape), every number is invented. No three.js in here, so the
// hero shell can import the port list without pulling in the 3D chunk.

export type Port = {
  id: string
  /** UN/LOCODE-style code (mock) */
  locode: string
  name: { en: string; si: string; ta: string }
  role: string
  /** harbour mouth: where the beacon stands */
  lon: number
  lat: number
  inPort: number
  arrivals24h: number
  draught: string
}

export const PORTS: Port[] = [
  { id: 'colombo', locode: 'LKCMB', name: { en: 'Colombo', si: 'කොළඹ', ta: 'கொழும்பு' }, role: 'Container hub', lon: 79.835, lat: 6.95, inPort: 42, arrivals24h: 31, draught: '18.0 m' },
  { id: 'hambantota', locode: 'LKHBA', name: { en: 'Hambantota', si: 'හම්බන්තොට', ta: 'அம்பாந்தோட்டை' }, role: 'Ro-ro and bunkering', lon: 81.11, lat: 6.115, inPort: 9, arrivals24h: 7, draught: '17.0 m' },
  { id: 'galle', locode: 'LKGAL', name: { en: 'Galle', si: 'ගාල්ල', ta: 'காலி' }, role: 'Regional port', lon: 80.225, lat: 6.025, inPort: 4, arrivals24h: 2, draught: '7.3 m' },
  { id: 'trincomalee', locode: 'LKTRR', name: { en: 'Trincomalee', si: 'ත්‍රිකුණාමලය', ta: 'திருகோணமலை' }, role: 'Natural harbour, bulk', lon: 81.27, lat: 8.545, inPort: 11, arrivals24h: 5, draught: '13.0 m' },
  { id: 'kankesanthurai', locode: 'LKKNK', name: { en: 'Kankesanthurai', si: 'කන්කසන්තුරේ', ta: 'காங்கேசன்துறை' }, role: 'Cargo and ferry', lon: 80.04, lat: 9.82, inPort: 3, arrivals24h: 2, draught: '8.0 m' },
  { id: 'oluvil', locode: 'LKOLU', name: { en: 'Oluvil', si: 'ඔලුවිල්', ta: 'ஒலுவில்' }, role: 'Fishery harbour', lon: 81.865, lat: 7.29, inPort: 2, arrivals24h: 1, draught: '8.0 m' },
  { id: 'point-pedro', locode: 'LKPPE', name: { en: 'Point Pedro', si: 'පේදුරු තුඩුව', ta: 'பருத்தித்துறை' }, role: 'Fishery harbour', lon: 80.235, lat: 9.835, inPort: 6, arrivals24h: 4, draught: '3.5 m' },
]

export type LaneKind = 'trunk' | 'feeder' | 'coastal'
export type LonLat = [number, number]
export type LaneDef = {
  id: string
  kind: LaneKind
  /** waypoints in travel order: light pulses run first → last */
  pts: LonLat[]
  /** fade the ribbon out over this many scene units at its open-sea ends */
  fade?: [number, number]
}

/** The East–West trunk: Suez and the Gulf to the west, Malacca to the east. */
const TRUNK: LonLat[] = [
  [68.0, 9.6], [72.0, 8.4], [75.5, 7.2], [77.6, 6.45], [79.2, 5.92], [80.0, 5.7], [80.59, 5.66], [81.3, 5.7], [82.4, 5.83], [84.5, 5.98], [88.0, 6.12], [93.0, 6.2],
]
/** Traffic separation: each direction keeps to its starboard side, ~2.5 nm off the centreline. */
const SEPARATION = 0.042
const offsetLat = (pts: LonLat[], d: number): LonLat[] => pts.map(([lon, lat]) => [lon, lat + d])

/** Coasting route, clockwise from Colombo round Dondra and up the east coast. The Palk Strait side is too shallow for ships, so there is no western leg to the north. */
const COASTAL: LonLat[] = [
  [79.76, 6.95], [79.8, 6.62], [79.87, 6.36], [79.96, 6.12], [80.14, 5.93], [80.36, 5.86], [80.6, 5.82], [80.86, 5.92], [81.13, 6.03], [81.42, 6.1], [81.7, 6.33],
  [81.9, 6.62], [81.97, 6.92], [81.97, 7.29], [81.93, 7.58], [81.82, 7.88], [81.62, 8.18], [81.46, 8.44], [81.38, 8.62], [81.27, 8.86], [81.09, 9.1], [80.92, 9.35],
  [80.68, 9.62], [80.42, 9.84], [80.23, 9.92], [80.04, 9.9],
]

export const LANES: LaneDef[] = [
  { id: 'trunk-east', kind: 'trunk', pts: offsetLat(TRUNK, -SEPARATION), fade: [7, 7] },
  { id: 'trunk-west', kind: 'trunk', pts: offsetLat(TRUNK, SEPARATION).reverse(), fade: [7, 7] },
  { id: 'colombo-west', kind: 'feeder', pts: [[77.6, 6.5], [78.6, 6.78], [79.35, 6.93], [79.76, 6.95]] },
  { id: 'colombo-south', kind: 'feeder', pts: [[80.0, 5.7], [79.74, 5.95], [79.62, 6.3], [79.62, 6.68], [79.76, 6.95]] },
  { id: 'hambantota-west', kind: 'feeder', pts: [[80.75, 5.67], [80.98, 5.82], [81.11, 6.02]] },
  { id: 'hambantota-east', kind: 'feeder', pts: [[81.55, 5.73], [81.3, 5.86], [81.13, 6.02]] },
  { id: 'coastal', kind: 'coastal', pts: COASTAL },
]

export type VesselKind = 'container' | 'tanker' | 'fishing'
/** Scene units per second. The hero runs the sea at roughly ×400: a 14-knot ship covers ~0.026 units a second. */
export const SPEED: Record<VesselKind, number> = { container: 0.026, tanker: 0.022, fishing: 0.011 }
/**
 * A sparse fleet: enough traffic to say "this is the busiest lane in the Indian Ocean",
 * few enough hulls that each one can be followed. Density falls with rank.
 */
export const FLEET: { lane: string; kind: VesselKind; count: number; bothWays?: boolean }[] = [
  { lane: 'trunk-east', kind: 'container', count: 6 },
  { lane: 'trunk-east', kind: 'tanker', count: 4 },
  { lane: 'trunk-west', kind: 'container', count: 6 },
  { lane: 'trunk-west', kind: 'tanker', count: 4 },
  { lane: 'colombo-west', kind: 'container', count: 2, bothWays: true },
  { lane: 'colombo-south', kind: 'container', count: 2, bothWays: true },
  { lane: 'hambantota-west', kind: 'tanker', count: 1, bothWays: true },
  { lane: 'hambantota-east', kind: 'container', count: 1, bothWays: true },
  { lane: 'coastal', kind: 'container', count: 4, bothWays: true },
  { lane: 'coastal', kind: 'fishing', count: 9, bothWays: true },
]
