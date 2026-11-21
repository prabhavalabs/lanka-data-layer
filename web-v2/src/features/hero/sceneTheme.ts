// Scene theme for the hero: one palette per page theme, shared by every layer of
// the 3D island (sky, sea, land, maritime, landmarks).
//
// ── For whoever builds the landmark models ────────────────────────────────────
// Read the theme one of two ways. Both are stable; nothing here imports three.js.
//
//   1. Inside the <Canvas> tree:  const theme = useSceneTheme()
//      The provider sits in HeroScene's <Scene>, so any component under it can
//      call the hook. It re-renders only when the page theme flips (dark ↔ light).
//
//   2. Outside React / for static lookups:  SCENE_THEMES.dark / SCENE_THEMES.light
//
// What a landmark should take from it:
//   theme.landmark.glow        0..1  how much emissive / window light to show.
//                                    1 at night, 0 by day. This is the same number
//                                    Landmarks already receives as its `glow` prop.
//   theme.landmark.stone/roof/metal/foliage/base
//                                    neutral material tints tuned to sit on this
//                                    theme's land ramp. Optional: use them where a
//                                    model has no colour of its own to defend.
//   theme.landmark.emissive          the one warm light colour for lit windows,
//                                    lamps and finials. Keep night lights to this
//                                    hue so the island does not turn into a fair.
//   theme.land.low / plain / hill / high / summit
//                                    the elevation ramp. `plain` is the ground most
//                                    models stand on; plinths and aprons start from it.
//   theme.ocean.shelf / mid / shoreline
//                                    water tones for moats, tanks and surf at a model's foot.
//   theme.light.*                    key direction, colour and intensity, if a
//                                    model wants a baked fake-AO or a rim that
//                                    agrees with the real lights.
//
// Every field named above is a stable contract: values get retuned, names do not go away.
//
// Rendering facts worth knowing:
//   - Tone mapping is THREE.NeutralToneMapping at exposure 1: albedo comes out
//     very close to the hex you author. No ACES desaturation to compensate for.
//   - Lit materials should stay matte: roughness 0.7–0.9, metalness 0 (≤ 0.4 for
//     the few gilded finials). The island itself is roughness 0.86.
//   - One shadow-casting key light. Night is a cool moon key, not a warm sunset,
//     so warm colour at night must come from emissive, never from albedo.
//   - Scene units: 1 unit = 1° of latitude. The land is a plate standing in the sea:
//     sea level is y = 0, the coastal plain is at y = PLINTH (0.024, exported from
//     terrain.ts) and meets the water over a rounded rim BEVEL (0.021°) wide. Always
//     place models with terrain.heightAt(lon, lat): it never returns less than PLINTH
//     on land, so a harbour model stands on the plate instead of sliding down its rim.
//   - Sri Pada is the highest point of the terrain (≈ 0.27 units, SUMMIT in terrain.ts);
//     the central plateau tops out near 0.2. Relief is exaggerated about ×12.
//   - A second, shadowless rim light comes from behind the island (theme.light.rim*).
//   - The sea is drawn first and writes no depth, so nothing can z-fight with it.
// ──────────────────────────────────────────────────────────────────────────────
import { createContext, useContext } from 'react'

export type SceneThemeName = 'dark' | 'light'

export type SceneTheme = {
  name: SceneThemeName
  sky: {
    /** top of the dome: equals the page background so the hero has no seam under the nav */
    zenith: string
    /** the band the sea dissolves into */
    horizon: string
    /** soft bloom on the horizon toward the key light */
    glow: string
    /** 0..1 strength of that bloom */
    glowAmount: number
    /** compass direction the bloom sits in (scene space, y ignored): behind the island, away from the headline */
    glowDir: [number, number, number]
  }
  light: {
    /** direction TO the key light (scene space: +y up, −z north, +x east) */
    keyDir: [number, number, number]
    keyColor: string
    keyIntensity: number
    /** a second, shadowless light from behind the island: picks out far slopes and the rim */
    rimDir: [number, number, number]
    rimColor: string
    rimIntensity: number
    hemiSky: string
    hemiGround: string
    hemiIntensity: number
    exposure: number
  }
  ocean: {
    deep: string
    mid: string
    /** the shelf: water over the shallows, hugging the coast */
    shelf: string
    /** what the surface reflects at grazing angles */
    reflect: string
    /** specular colour of the key light on the water */
    glint: string
    glintAmount: number
    /** the palest water tone: surf at the foot of the rim, and the tint of the survey graticule */
    shoreline: string
    /** half-degree graticule strength */
    grid: number
  }
  land: {
    /** elevation ramp, sea level → summit. Albedo, before lighting. */
    low: string
    plain: string
    hill: string
    high: string
    summit: string
    /** the foot of the plate's rounded edge, at the waterline: this band is the shoreline */
    rim: string
    /** contour line colour and strength (0 = none) */
    contour: string
    contourOpacity: number
    river: string
    riverOpacity: number
    rail: string
  }
  maritime: {
    /** every lane is this one hue; rank is carried by weight, never by colour */
    lane: string
    /** line opacity by rank */
    trunk: number
    feeder: number
    coastal: number
    /** the short ties from the coasting route into each harbour */
    spur: number
    pulse: string
    vessel: string
    /** hairline under a vessel glyph so it holds on light water */
    vesselEdge: string
    port: string
    portCore: string
    /** the hovered / selected port: the page's rose accent */
    portActive: string
  }
  label: {
    ink: string
    muted: string
    halo: string
  }
  landmark: {
    glow: number
    stone: string
    roof: string
    metal: string
    foliage: string
    base: string
    emissive: string
  }
  population: {
    /** sequential ramp, low → high */
    ramp: [string, string, string, string]
    emissive: string
  }
}

// Night: a clean deep-navy sea under a cool, near-white moon key. The land is the
// brightest large shape in the frame and the only green one; warm colour is reserved
// for lit landmarks and the rose accent, so both read instantly.
const dark: SceneTheme = {
  name: 'dark',
  sky: { zenith: '#0b0e12', horizon: '#18364f', glow: '#4d8cb0', glowAmount: 0.34, glowDir: [0.55, 0, -0.84] },
  light: {
    keyDir: [-0.74, 0.62, 0.22],
    keyColor: '#f3f6ff',
    keyIntensity: 3.8,
    rimDir: [0.72, 0.34, -0.6],
    rimColor: '#c4e4ec',
    rimIntensity: 0.55,
    hemiSky: '#a3b9d3',
    hemiGround: '#26353f',
    hemiIntensity: 1.0,
    exposure: 1,
  },
  ocean: {
    deep: '#08131f',
    mid: '#0e2841',
    shelf: '#1b5169',
    reflect: '#30597a',
    glint: '#d6e8f5',
    glintAmount: 0.3,
    shoreline: '#c9e9ee',
    grid: 0.026,
  },
  land: {
    low: '#5a9870',
    plain: '#6aa67a',
    hill: '#98c39a',
    high: '#cbdcb8',
    summit: '#f5f3e2',
    rim: '#c2dccd',
    contour: '#f2fff6',
    contourOpacity: 0.2,
    river: '#b9e6f0',
    riverOpacity: 0.2,
    rail: '#ffc27a',
  },
  maritime: {
    lane: '#cfeaf2',
    trunk: 0.9,
    feeder: 0.56,
    coastal: 0.4,
    spur: 0.26,
    pulse: '#ffffff',
    vessel: '#f4fbfd',
    vesselEdge: '#06121c',
    port: '#f0668a',
    portCore: '#ffffff',
    portActive: '#ff8fae',
  },
  label: { ink: '#e8edf2', muted: '#9aa7b4', halo: '#0b0e12' },
  landmark: { glow: 1, stone: '#d5d6cc', roof: '#b06a5c', metal: '#e6c26a', foliage: '#4f8f68', base: '#6aa67a', emissive: '#ffd49a' },
  population: { ramp: ['#5b4fc4', '#7f6ee0', '#a797f0', '#d6cdfb'], emissive: '#5a45bd' },
}

// Day: high, soft sun; a clear mid-blue sea so the pale island stays the figure. The sky
// is a studio backdrop rather than a sky: page white overhead, a pale blue haze at the horizon.
const light: SceneTheme = {
  name: 'light',
  sky: { zenith: '#f7f9fb', horizon: '#cfe4f4', glow: '#eaf4fb', glowAmount: 0.4, glowDir: [0.55, 0, -0.84] },
  light: {
    keyDir: [-0.66, 0.68, 0.3],
    keyColor: '#fff7ea',
    keyIntensity: 3.4,
    rimDir: [0.72, 0.34, -0.6],
    rimColor: '#ffffff',
    rimIntensity: 0.3,
    hemiSky: '#dcebff',
    hemiGround: '#8a9a7c',
    hemiIntensity: 1.2,
    exposure: 1,
  },
  ocean: {
    deep: '#1c69a0',
    mid: '#2e83bc',
    shelf: '#78c6e2',
    reflect: '#96c8ea',
    glint: '#ffffff',
    glintAmount: 0.4,
    shoreline: '#ffffff',
    grid: 0.05,
  },
  land: {
    low: '#9bcd8e',
    plain: '#aedb9c',
    hill: '#c9e2ae',
    high: '#e4eac9',
    summit: '#fbf8ec',
    rim: '#fbf3dc',
    contour: '#2c5a3c',
    contourOpacity: 0.16,
    river: '#4f9fd6',
    riverOpacity: 0.38,
    rail: '#b84a12',
  },
  maritime: {
    lane: '#ffffff',
    trunk: 0.94,
    feeder: 0.66,
    coastal: 0.5,
    spur: 0.36,
    pulse: '#ffffff',
    vessel: '#ffffff',
    vesselEdge: '#0e3f63',
    port: '#c22f5a',
    portCore: '#ffffff',
    portActive: '#c22f5a',
  },
  label: { ink: '#14181c', muted: '#515c66', halo: '#fbfbfa' },
  landmark: { glow: 0, stone: '#ece8dc', roof: '#b9654f', metal: '#e2b64c', foliage: '#5d9a63', base: '#aedb9c', emissive: '#ffd49a' },
  population: { ramp: ['#6650cf', '#8870e6', '#ad9bf2', '#d9d0fb'], emissive: '#6046c0' },
}

export const SCENE_THEMES: Record<SceneThemeName, SceneTheme> = { dark, light }

export const SceneThemeContext = createContext<SceneTheme>(dark)
/** The active scene theme. Only valid under HeroScene's <Scene>. */
export const useSceneTheme = () => useContext(SceneThemeContext)
