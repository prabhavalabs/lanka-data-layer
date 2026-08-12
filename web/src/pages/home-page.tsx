import * as React from "react";
import { Link } from "react-router-dom";

import { MiniMap } from "@/components/minimap/mini-map";
import { buildMiniMapScene } from "@/components/minimap/scene";
import { apiGetPayload } from "@/lib/api";
import { docsEndpointsByGroup, type DocsGroup } from "@/lib/docs-spec";
import { JsonViewer } from "@/pages/docs/components/json-viewer";

const GITHUB_URL = "https://github.com/prabhavalabs/lanka-data-layer";

const GROUP_DESCRIPTIONS: Record<DocsGroup, string> = {
  "Search & Lookup": "Typeahead, ranked matches, reverse geocoding and coordinate parsing in one family of endpoints.",
  "Admin & Geometry": "Every unit from GN division to province, with boundaries to match.",
  Population: "WorldPop rasters served as points, radii and island-wide grids.",
  Elections: "Official results by division, digitised from Election Commission gazettes.",
  "Data & Tiles": "Liveness, the full dataset catalog, and vector tiles for your own basemaps.",
};

const CREDITS = [
  { name: "geoBoundaries", lic: "CC BY 3.0 IGO" },
  { name: "OCHA COD-AB + cod-ps", lic: "CC BY-IGO" },
  { name: "OpenStreetMap", lic: "ODbL" },
  { name: "GeoNames", lic: "CC BY 4.0" },
  { name: "WorldPop", lic: "CC BY 4.0" },
  { name: "Election Commission (via nuuuwan)", lic: "open" },
];

/** Real shape captured from a running GET /v1/reverse?lat=6.9344&lon=79.8428 (docs-spec.ts's own "reverse" example) — used only when the live fetch below fails, so the quickstart panel never shows a blank/broken state. */
const FALLBACK_REVERSE_PAYLOAD = {
  cell_id: 8294861,
  gnd: {
    pcode: "LK1103140",
    level: 4,
    name: "Suduwella",
    name_si: "සුදුවැල්ල",
    name_ta: null,
    parent_pcode: "LK1103",
    area_km2: 0.9221773,
    centroid: { lat: 6.92655401, lon: 79.85914308 },
  },
  ds_division: { pcode: "LK1103", level: 3, name: "Colombo" },
  district: { pcode: "LK11", level: 2, name: "Colombo District" },
  province: { pcode: "LK1", level: 1, name: "Western Province" },
  postal_code: "00200",
  nearest_place: { id: 446, name: "Suduwella", distance_m: 135 },
};

type QuickstartState = { status: "loading" } | { status: "live"; value: unknown } | { status: "fallback" };

function useQuickstartReverse(): QuickstartState {
  const [state, setState] = React.useState<QuickstartState>({ status: "loading" });
  React.useEffect(() => {
    const controller = new AbortController();
    apiGetPayload("/reverse", { params: { lat: 6.9344, lon: 79.8428 }, signal: controller.signal })
      .then((value) => setState({ status: "live", value }))
      .catch((err: unknown) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
        setState({ status: "fallback" });
      });
    return () => controller.abort();
  }, []);
  return state;
}

function useGridScene() {
  const [scene, setScene] = React.useState<ReturnType<typeof buildMiniMapScene> | null>(null);
  React.useEffect(() => {
    const controller = new AbortController();
    apiGetPayload("/population/grid", { params: { res: 0.05 }, signal: controller.signal })
      .then((payload) => setScene(buildMiniMapScene("population-grid", payload, {})))
      .catch((err: unknown) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
      });
    return () => controller.abort();
  }, []);
  return scene;
}

function CopyButton({ text, className }: { text: string; className?: string }) {
  const [copied, setCopied] = React.useState(false);
  return (
    <button
      type="button"
      onClick={() => {
        void navigator.clipboard.writeText(text).catch(() => {});
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1400);
      }}
      className={className}
    >
      {copied ? "copied ✓" : "copy"}
    </button>
  );
}

export function HomePage() {
  const quickstart = useQuickstartReverse();
  const gridScene = useGridScene();
  const groups = docsEndpointsByGroup();
  const curl = 'curl "/v1/reverse?lat=6.9344&lon=79.8428"';
  const quickstartValue = quickstart.status === "live" ? quickstart.value : FALLBACK_REVERSE_PAYLOAD;

  return (
    <div className="h-full min-w-0 overflow-y-auto">
      <div className="mx-auto max-w-[920px] px-6 py-12 sm:px-12 sm:py-16">
        <div className="flex flex-col items-stretch gap-10 lg:flex-row">
          <div className="flex min-w-0 flex-1 flex-col justify-center">
            <div className="mb-[18px] inline-flex items-center gap-2 font-mono text-[11px] text-brand">
              <span className="h-px w-4 bg-brand" />
              FREE · OPEN SOURCE · NO API KEYS
            </div>
            <h1 className="mb-3.5 text-[34px] leading-[1.15] font-bold tracking-[-0.022em] text-wrap-pretty">
              Sri Lanka&rsquo;s open geo-data API
            </h1>
            <p className="mb-6 text-[15.5px] leading-[1.6] text-ink2 text-wrap-pretty">
              Reverse geocoding, place search, postal codes, population and election results — for every GN division, DS
              division, district and province on the island.
            </p>
            <div className="flex flex-wrap gap-2.5">
              <Link
                to="/docs/lookup"
                className="rounded-lg bg-brand px-4 py-[9px] text-[13.5px] font-semibold text-white transition-colors hover:bg-brand2"
              >
                Make your first request
              </Link>
              <Link
                to="/map"
                className="rounded-lg border border-line2 px-4 py-[9px] text-[13.5px] font-medium text-ink transition-colors hover:border-brand"
              >
                Explore the map ↗
              </Link>
            </div>
          </div>

          <div className="relative w-full shrink-0 overflow-hidden rounded-xl border border-border bg-bg2 lg:w-[320px]">
            <MiniMap scene={gridScene} className="min-h-[290px] w-full" />
            <div className="absolute bottom-2.5 left-2.5 rounded-md border border-border bg-bg2 px-2 py-[3px] font-mono text-[10px] text-ink3">
              /v1/population/grid · live
            </div>
          </div>
        </div>

        <div className="mt-14">
          <div className="mb-3 text-[11px] font-semibold uppercase tracking-[0.09em] text-ink3">Quickstart</div>
          <div className="overflow-hidden rounded-xl border border-border bg-code-bg">
            <div className="flex items-center gap-2 border-b border-white/10 px-3.5 py-[9px]">
              <span className="rounded-[4px] bg-positive-soft px-[6px] py-[2px] font-mono text-[9.5px] font-semibold text-positive">GET</span>
              <span className="font-mono text-[12px] text-code-ink2">Where am I? One request, no key.</span>
              <CopyButton
                text={curl}
                className="ml-auto rounded-md border border-white/15 px-[9px] py-[3px] font-mono text-[10.5px] text-code-ink2 transition-colors hover:text-code-ink"
              />
            </div>
            <div className="overflow-x-auto px-4 py-3.5 font-mono text-[12.5px] leading-[1.65] text-code-ink">
              <span className="text-json-punc">$</span> curl &quot;<span className="text-json-str">/v1/reverse?lat=6.9344&amp;lon=79.8428</span>&quot;
            </div>
            <div className="border-t border-white/10 px-4 py-3.5">
              {quickstart.status === "loading" ? (
                <div className="font-mono text-[12px] text-code-ink3">Loading…</div>
              ) : (
                <JsonViewer value={quickstartValue} className="border-0 bg-transparent p-0" />
              )}
              {quickstart.status === "fallback" && (
                <div className="mt-2 font-mono text-[10.5px] text-warn">API unreachable — showing a captured example response.</div>
              )}
            </div>
          </div>
          <div className="mt-2.5 text-[12.5px] text-ink3">
            Every response is JSON, cached with ETags, and free for any use with attribution.{" "}
            <Link to="/docs/lookup" className="text-brand hover:text-brand2 hover:underline">
              Try it live in the docs →
            </Link>
          </div>
        </div>

        <div className="mt-14">
          <div className="mb-3 text-[11px] font-semibold uppercase tracking-[0.09em] text-ink3">What&rsquo;s in the box</div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {groups.map(({ group, endpoints }) => (
              <Link
                key={group}
                to={`/docs/${endpoints[0]?.slug ?? ""}`}
                className="flex min-h-[118px] flex-col gap-2 rounded-xl border border-border bg-bg2 p-4 transition-colors hover:border-brand hover:bg-bg3"
              >
                <div className="font-mono text-[10.5px] text-brand">
                  {endpoints.length} endpoint{endpoints.length === 1 ? "" : "s"}
                </div>
                <div className="text-[14.5px] font-semibold tracking-[-0.01em]">{group}</div>
                <div className="text-[12.5px] leading-[1.5] text-ink2 text-wrap-pretty">{GROUP_DESCRIPTIONS[group]}</div>
              </Link>
            ))}
            <Link
              to="/map"
              className="flex min-h-[118px] flex-col gap-2 rounded-xl border border-border bg-bg2 p-4 transition-colors hover:border-brand hover:bg-bg3"
            >
              <div className="font-mono text-[10.5px] text-brand">flagship</div>
              <div className="text-[14.5px] font-semibold tracking-[-0.01em]">Explore Map</div>
              <div className="text-[12.5px] leading-[1.5] text-ink2 text-wrap-pretty">The whole island live — boundaries, layers, 3D population.</div>
            </Link>
          </div>
        </div>

        <div className="mt-16 border-t border-border pt-7">
          <div className="flex flex-wrap items-baseline gap-3.5">
            <div className="text-[13px] font-semibold">Built on open data</div>
            <div className="text-[12px] text-ink3">Geopub redistributes and refines these datasets. Keep their credit lines when you build.</div>
          </div>
          <div className="mt-3.5 flex flex-wrap gap-2">
            {CREDITS.map((c) => (
              <span key={c.name} className="inline-flex items-center gap-1.5 rounded-full border border-border bg-bg2 px-3 py-[5px] text-[12px] text-ink2">
                <span className="font-medium text-ink">{c.name}</span>
                <span className="font-mono text-[10px] text-ink3">{c.lic}</span>
              </span>
            ))}
          </div>
          <div className="mt-5 flex flex-wrap items-center gap-2 text-[12px] text-ink3">
            <span>© 2026 Geopub contributors</span>
            <span>·</span>
            <a href={GITHUB_URL} target="_blank" rel="noreferrer" className="text-brand hover:text-brand2 hover:underline">
              GitHub
            </a>
            <span>·</span>
            <Link to="/datasets" className="text-brand hover:text-brand2 hover:underline">
              Dataset downloads
            </Link>
            <span>·</span>
            <a href="/v1/health" target="_blank" rel="noreferrer" className="text-brand hover:text-brand2 hover:underline">
              Status
            </a>
          </div>
        </div>
      </div>
    </div>
  );
}
