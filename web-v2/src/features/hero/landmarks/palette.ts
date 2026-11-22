// Landmark colours as named tokens, resolved once per scene theme.
//
// Two kinds of token:
//   - site tokens (ground, foliage, stone, roof, gold, water …) come straight from
//     sceneTheme.ts, so a model's lawn, trees and plinth sit on that theme's land
//     ramp and follow it when the palette is retuned;
//   - signature tokens (Sigiriya's ochre gneiss, the Lotus Tower's magenta petals,
//     the S12's blue …) are the colours a landmark is recognised by. They are
//     authored per theme: the night values are a touch cooler and lower in chroma
//     because the night key is a cool moon and warmth has to come from emissive.
import * as THREE from 'three'
import { SCENE_THEMES, type SceneTheme, type SceneThemeName } from '../sceneTheme'

const mix = (a: string, b: string, t: number) => '#' + new THREE.Color(a).lerp(new THREE.Color(b), t).getHexString()
const mul = (a: string, k: number) => '#' + new THREE.Color(a).multiplyScalar(k).getHexString()

function tokens(th: SceneTheme) {
  const night = th.name === 'dark'
  /** signature colour: day value, optional night value */
  const sig = (day: string, dark?: string) => (night ? (dark ?? day) : day)
  const L = th.landmark, land = th.land
  return {
    /* ---- site: from the scene theme ---- */
    ground: L.base,
    groundLow: land.low,
    plain: land.plain,
    hill: land.hill,
    high: land.high,
    summit: land.summit,
    foliage: L.foliage,
    foliageDeep: mul(L.foliage, 0.74),
    foliageLite: mix(L.foliage, land.hill, 0.5),
    lawn: mix(L.base, land.hill, 0.45),
    tea: mix(L.foliage, land.plain, 0.55),
    stone: L.stone,
    stoneDark: mul(L.stone, 0.66),
    stoneWarm: mix(L.stone, sig('#c9b089', '#b3ab98'), 0.6),
    roof: L.roof,
    roofDark: mul(L.roof, 0.72),
    /** flat clay tile, weathered brown: Kandyan ranges */
    tile: mix(L.roof, sig('#5f4a3e', '#51494a'), 0.55),
    gold: L.metal,
    goldDeep: mul(L.metal, 0.78),
    water: mix(th.ocean.shelf, land.river, night ? 0.3 : 0.45),
    waterDeep: mix(th.ocean.mid, th.ocean.shelf, 0.5),
    sea: th.ocean.shelf,
    foam: th.ocean.shoreline,
    light: L.emissive,

    /* ---- shared building materials ---- */
    white: sig('#fbf9f2', '#eef0ec'),
    whiteShade: sig('#e4e0d4', '#cfd4d0'),
    cream: sig('#f1e6c8', '#dcd8c6'),
    sand: sig('#e6d6ae', '#b9c0a6'),
    path: sig('#d9bf94', '#b3b29a'),
    brick: sig('#b5694a', '#9a665a'),
    brickDark: sig('#8e4c36', '#7a4e48'),
    timber: sig('#6d4a35', '#5a463e'),
    asphalt: sig('#6f7275', '#5c666c'),
    iron: sig('#3d4248', '#353d45'),
    glass: sig('#9fc4d8', '#7fa6bd'),
    palmTrunk: sig('#9a8466', '#7f7a68'),
    palmLeaf: sig('#4f9a4f', '#3f8166'),
    red: sig('#c2402f', '#a84a44'),

    /* ---- Sigiriya: iron-stained gneiss ---- */
    // The one signature colour that keeps its chroma at night. The moon key is cool and the rock's walls face away
    // from it under their own overhang: at the usual night values the ochre fell to a dull brown, so these stay warm
    // and the rock adds a little light of its own (see `glow` in the model).
    sigOchre: sig('#cf9460', '#cf9a62'),
    sigRust: sig('#b8683c', '#b4683e'),
    sigPale: sig('#e2bd8c', '#e6c594'),
    sigStreak: sig('#5a4b45', '#54443c'),
    sigGrey: sig('#8f8478', '#958a7c'),
    sigMirror: sig('#eab56e', '#eab871'),

    /* ---- Sri Pada ---- */
    peakRock: sig('#8d8a7c', '#7d8580'),
    peakRockLite: sig('#b3ad9a', '#a3aaa2'),

    /* ---- Nine Arch ---- */
    viaduct: sig('#b9a88c', '#a3a092'),
    viaductDark: sig('#7f7362', '#6c6f69'),
    ballast: sig('#8a6a50', '#77695e'),
    trainBlue: sig('#2a62b8', '#3a66ad'),
    trainBlueDeep: sig('#1c458c', '#294a86'),
    trainStripe: sig('#f2efe4', '#e2e6e4'),
    trainRoof: sig('#c9ced2', '#aeb8be'),

    /* ---- Lotus Tower ---- */
    lotusStem: sig('#8fb89a', '#7fa894'),
    lotusCalyx: sig('#3f9a62', '#3c8c68'),
    lotusPetal: sig('#c0489c', '#b4509e'),
    lotusPetalLite: sig('#e88fc6', '#d68cc4'),
    lotusPodium: sig('#b56aa8', '#9e6aa2'),
    lotusGold: sig('#e8c75a', '#d9c06a'),

    /* ---- Nallur ---- */
    gopuram: sig('#e3a23a', '#cf9f4e'),
    gopuramDeep: sig('#b9741f', '#a67a3c'),
    gopuramLite: sig('#f3c765', '#e2c276'),
    kovilRed: sig('#b83a2c', '#a04a44'),
    kovilWhite: sig('#f7f3e8', '#e6e8e2'),

    /* ---- Trincomalee ---- */
    cliff: sig('#a58d6f', '#8f8a7c'),
    cliffDark: sig('#6e5d4c', '#5d5e58'),
    cliffLite: sig('#c9b08a', '#b0ab98'),
    shiva: sig('#e9b79a', '#d6b0a0'),
    koneBlue: sig('#4f9ac4', '#4a8cb4'),
    konePink: sig('#e07f9a', '#cc7f98'),
    koneGreen: sig('#5fae7a', '#55a07c'),
    navy: sig('#7d8a94', '#6d7c88'),
    navyDark: sig('#4f5b66', '#46535f'),

    /* ---- Galle ---- */
    rampart: sig('#8c8274', '#787a74'),
    rampartDark: sig('#5f574d', '#525651'),
    ochreWall: sig('#e2b866', '#c9ae78'),
  }
}

export type Tok = keyof ReturnType<typeof tokens>

function resolve(name: SceneThemeName) {
  const t = tokens(SCENE_THEMES[name])
  const out = {} as Record<Tok, THREE.Color>
  for (const key of Object.keys(t) as Tok[]) out[key] = new THREE.Color(t[key])
  return out
}

/** linear-space colours per theme, indexed by token */
export const PALETTES: Record<SceneThemeName, Record<Tok, THREE.Color>> = { dark: resolve('dark'), light: resolve('light') }
