import type { ModelDef } from '../types'
import { buildGalle } from './galle'
import { buildLotusTower } from './lotusTower'
import { buildNallur } from './nallur'
import { buildNineArch } from './nineArch'
import { buildRuwanwelisaya } from './ruwanwelisaya'
import { buildSigiriya } from './sigiriya'
import { buildSriPada } from './sriPada'
import { buildTempleTooth } from './templeTooth'
import { buildTrincomalee } from './trincomalee'

/** keyed by landmark id (see LANDMARKS in the API mocks) */
export const MODELS: Record<string, ModelDef> = {
  sigiriya: {
    id: 'sigiriya',
    scale: 0.9,
    build: buildSigiriya,
    yaw: 0.55,
    notes: [
      '≈180 m gneiss monolith, plan ≈350 × 190 m on a N–S axis',
      'near-vertical walls, mushroom overhang; west face leans out over the Mirror Wall gallery',
      'ochre and rust rock, dark water streaks from the rim',
      'summit: stepped brick palace terraces, highest NW, rock-cut pool',
      'Lion Terrace halfway up the north end: two paws, brick stair, iron flights',
      'Water Gardens: moated, symmetric precinct on the west axis',
    ],
  },
  'sri-pada': {
    id: 'sri-pada',
    scale: 0.8,
    build: buildSriPada,
    yaw: 0.1,
    pop: 0,
    seat: 'rim',
    notes: [
      '2,243 m: a lone, steep cone with a needle summit on forested shoulders',
      'bare gneiss on the summit cone, montane forest below',
      'walled summit platform: footprint shrine, Saman shrine, bell',
      'Hatton stair zig-zags up the cone, lamp-lit all night in season',
      'white Peace Pagoda and the Nallathanni trailhead at its foot',
    ],
  },
  'lotus-tower': {
    id: 'lotus-tower',
    scale: 0.95,
    inland: 0.066,
    build: buildLotusTower,
    yaw: 0.2,
    notes: [
      '351.5 m; slender green shaft tapering from a lotus-throne podium',
      'bud at ≈60–75 % of the height: green sepals, magenta-pink petals',
      'short crown, then a long white mast: the top quarter',
      'six-floor podium of two inverted trapezoids with petal fins',
      'beside Beira Lake; lit in its own colours at night',
    ],
  },
  ruwanwelisaya: {
    id: 'ruwanwelisaya',
    scale: 0.9,
    build: buildRuwanwelisaya,
    yaw: 0.5,
    notes: [
      '103 m high, ≈92 m across: height ≈ 1.12 × diameter',
      'white bubble dome on three basal rings, red band on the lowest',
      'square harmika, devata kotuwa, ringed spire, gilded pinnacle with crystal',
      'vahalkada frontispieces at the four cardinal points',
      'square platform walled with a parade of elephants; corner stupas',
    ],
  },
  'temple-tooth': {
    id: 'temple-tooth',
    scale: 0.85,
    seat: 'high',
    core: 0.2,
    build: buildTempleTooth,
    yaw: 0.3,
    notes: [
      'Pattirippuwa: white octagon in the moat, open upper storey, red-tile Kandyan roof',
      'cloud wall (walakulu bemma) along the moat and the lake shore',
      'white two-storey ranges under dark Kandyan hipped roofs',
      'golden canopy (1987) standing over the inner shrine',
      'audience hall of timber posts; palace; wooded hill behind',
      'Kandy Lake in front with its island',
    ],
  },
  'galle-fort': {
    id: 'galle-fort',
    scale: 0.9,
    build: buildGalle,
    shore: 0.02,
    view: -20,
    notes: [
      'white, slightly tapered round tower, 26.5 m: gallery, glazed lantern, white cap',
      'on an ochre plinth on Point Utrecht Bastion, ringed by coconut palms',
      'dark battered Dutch ramparts, grassed terreplein, black rocks and surf',
      'white Meeran Mosque with baroque gables beside it',
      'red-roofed Dutch houses; clock tower on the landward wall',
    ],
  },
  'nine-arch': {
    id: 'nine-arch',
    scale: 0.72,
    build: buildNineArch,
    yaw: 0.35,
    notes: [
      '91 m long, 24 m high, 7.6 m wide; stone, brick and cement, no steel',
      'exactly nine semicircular arches on tall, slightly battered piers',
      'on a nine-degree curve: the deck sweeps ≈27°',
      'arch rings, string course, parapet; grey-brown weathered stone',
      'jungle ravine between tea hills; tunnels just beyond',
      'blue Class S12 set crossing',
    ],
  },
  nallur: {
    id: 'nallur',
    scale: 0.8,
    inland: 0.12,
    build: buildNallur,
    yaw: 0.45,
    notes: [
      'east front: five-storey golden gopuram behind a gilded arched porch',
      'nine-storey raja gopurams on the south and (slightly larger) north',
      'stepped storeys, projecting cornices, barrel vault with a row of kalasams',
      'red-and-white striped compound wall, domed bell towers',
      'flat-roofed halls, gilded vimana, holy pond in the southern court',
      'white sand, palmyra palms',
    ],
  },
  trinco: {
    id: 'trinco',
    scale: 0.95,
    build: buildTrincomalee,
    shore: 0,
    // the simplified coast has no inner harbour here: swing the rock square to the shore and put the bay on its open side
    yaw: 0.6,
    mirror: true,
    view: 150,
    notes: [
      'Swami Rock: a narrow promontory climbing to a ≈120 m sea cliff',
      'layered tan rock on the seaward face, scrub on the harbour side',
      "Fort Frederick's curtain wall and gate across the neck",
      'Koneswaram at the tip: pastel gopuram, mandapam, the seated Shiva',
      'natural harbour in the lee: quay, oil tanks, navy ship at anchor',
      'white lighthouse on Round Island',
    ],
  },
}
