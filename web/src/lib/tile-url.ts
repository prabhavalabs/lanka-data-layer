/**
 * Absolute URL to a `/v1/tiles/<layer>.pmtiles` archive (docs/architecture.md
 * §4: "the api serves ... as static files with range-request support ...
 * clients use the pmtiles protocol, no per-tile endpoint"). Must be absolute
 * — the pmtiles:// custom protocol hands its suffix straight to `fetch()`,
 * so a relative path wouldn't resolve against the current page the way a
 * normal `src`/`href` would. Works unmodified against both the Vite dev
 * proxy (same origin, see vite.config.ts) and the prod nginx vhost.
 */
export function pmtilesUrl(layer: string): string {
  return `pmtiles://${window.location.origin}/v1/tiles/${layer}.pmtiles`;
}
