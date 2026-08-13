/**
 * Population density ramp used by both the docs-design mocks (minimap.html
 * and explore-map.html), per scratchpad/design/DESIGN-NOTES.md: "Population
 * color ramp (both mocks): #41102382 → #8D153A → #C73E3E → #EF8A2C →
 * #FFD166." This is deliberately a different ramp from the flagship map's
 * dark/light ramps in population-layer.ts — that one is tuned for a
 * full-screen 3D column layer; this one is the small, flat-map ramp the
 * design specifies for MiniMap's population/grid views specifically, so it
 * intentionally does not import from population-layer.ts.
 */

interface RgbaStop {
  r: number;
  g: number;
  b: number;
  a: number;
}

function parseHex(hex: string): RgbaStop {
  const clean = hex.replace("#", "");
  const r = parseInt(clean.slice(0, 2), 16);
  const g = parseInt(clean.slice(2, 4), 16);
  const b = parseInt(clean.slice(4, 6), 16);
  const a = clean.length >= 8 ? parseInt(clean.slice(6, 8), 16) / 255 : 1;
  return { r, g, b, a };
}

const STOPS: RgbaStop[] = ["#41102382", "#8D153A", "#C73E3E", "#EF8A2C", "#FFD166"].map(parseHex);

/** Interpolates the population ramp at `t` (0-1) and returns a CSS rgba() string. */
export function populationRampColor(t: number): string {
  const clamped = Math.min(1, Math.max(0, t));
  const scaled = clamped * (STOPS.length - 1);
  const i = Math.min(STOPS.length - 2, Math.floor(scaled));
  const f = scaled - i;
  const a = STOPS[i];
  const b = STOPS[i + 1];
  const r = Math.round(a.r + (b.r - a.r) * f);
  const g = Math.round(a.g + (b.g - a.g) * f);
  const bl = Math.round(a.b + (b.b - a.b) * f);
  const alpha = a.a + (b.a - a.a) * f;
  return `rgba(${r}, ${g}, ${bl}, ${alpha.toFixed(3)})`;
}

/** log1p-scaled 0-1 position for a population value against a domain ceiling — spreads the right-skewed distribution instead of crushing everything into the coolest bucket. */
export function populationRampT(value: number, domainMax: number): number {
  if (domainMax <= 0) return 0;
  return Math.log1p(Math.max(0, value)) / Math.log1p(domainMax);
}

/** Party colors for the elections minimap view — falls back to a neutral gray when the API didn't supply a color for that party. */
export function partyColor(color: string | null | undefined): string {
  return color && /^#[0-9a-fA-F]{3,8}$/.test(color) ? color : "#8a96a1";
}
