import * as React from "react";
import { Map as MaplibreMap, Marker } from "maplibre-gl";
import type { GeoJSONSource, StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";

import { useTheme } from "@/components/theme-provider";
import { apiGetRaw } from "@/lib/api";
import { cn } from "@/lib/utils";
import "@/components/map/pmtiles-protocol";
import { pmtilesUrl } from "@/lib/tile-url";
import { circleFeature } from "@/components/minimap/circle";
import { populationRampColor } from "@/components/minimap/ramp";
import type { MiniMapScene } from "@/components/minimap/scene";

type MiniMapTheme = "dark" | "light";

/**
 * The MiniMap's own flat-cartographic palette — deliberately distinct from
 * the app chrome's --bg/--bg2/etc tokens (per DESIGN-NOTES.md's "Minimap
 * mock" section, which specs its own water/land/coast/line/marker/glow
 * colors independent of the sidebar/docs palette).
 */
const MM_THEME: Record<MiniMapTheme, { water: string; land: string; coast: string; line: string; accent: string; marker: string; glowFill: string; chipBg: string; chipText: string }> = {
  dark: {
    water: "#0A0D11",
    land: "#141A21",
    coast: "#2B3742",
    line: "#232D37",
    accent: "#E5537A",
    marker: "#FF6B93",
    glowFill: "rgba(229,83,122,0.10)",
    chipBg: "rgba(13,17,22,0.85)",
    chipText: "#94A1AE",
  },
  light: {
    water: "#EAEFF3",
    land: "#FFFFFF",
    coast: "#C6CFD8",
    line: "#E2E8ED",
    accent: "#8D153A",
    marker: "#8D153A",
    glowFill: "rgba(141,21,58,0.07)",
    chipBg: "rgba(255,255,255,0.92)",
    chipText: "#5A6672",
  },
};

const EMPTY_FC: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

const SRC_LAND = "mm-land";
const SRC_BOUNDARY = "mm-boundary";
const SRC_CIRCLE = "mm-circle";
const SRC_DOTS = "mm-dots";
const SRC_TILE = "mm-tile";

function baseStyle(theme: MiniMapTheme): StyleSpecification {
  const t = MM_THEME[theme];
  return {
    version: 8,
    sources: {
      [SRC_LAND]: { type: "vector", url: pmtilesUrl("admin") },
    },
    layers: [
      { id: "mm-bg", type: "background", paint: { "background-color": t.water } },
      { id: "mm-land-fill", type: "fill", source: SRC_LAND, "source-layer": "adm1", paint: { "fill-color": t.land } },
      { id: "mm-district-line", type: "line", source: SRC_LAND, "source-layer": "adm2", paint: { "line-color": t.line, "line-width": 0.5, "line-opacity": 0.7 } },
      { id: "mm-coast-line", type: "line", source: SRC_LAND, "source-layer": "adm1", paint: { "line-color": t.coast, "line-width": 1 } },
    ],
  };
}

const REGION_FILL_RADIUS_KM = 42;

function reducedMotionQuery(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function buildMarkerElement(opts: { primary: boolean; color: string; reducedMotion: boolean }): HTMLDivElement {
  const wrap = document.createElement("div");
  wrap.className = "relative flex items-center justify-center";
  const size = opts.primary ? 12 : 7;
  wrap.style.width = `${size}px`;
  wrap.style.height = `${size}px`;

  if (!opts.reducedMotion) {
    const ring = document.createElement("span");
    ring.className = "absolute inline-block rounded-full";
    ring.style.width = "100%";
    ring.style.height = "100%";
    ring.style.background = opts.color;
    ring.style.animation = "marker-ring 2.2s ease-out infinite";
    wrap.append(ring);
  }
  const halo = document.createElement("span");
  halo.className = "absolute rounded-full";
  halo.style.inset = "-3px";
  halo.style.background = opts.color;
  halo.style.opacity = "0.18";
  const dot = document.createElement("span");
  dot.className = "relative inline-block rounded-full ring-2 ring-white/80";
  dot.style.width = "100%";
  dot.style.height = "100%";
  dot.style.background = opts.color;
  wrap.append(halo, dot);
  return wrap;
}

function buildChipElement(text: string, theme: MiniMapTheme): HTMLDivElement {
  const t = MM_THEME[theme];
  const el = document.createElement("div");
  el.className = "whitespace-nowrap rounded-full font-mono";
  el.style.fontSize = "10px";
  el.style.padding = "3px 8px";
  el.style.background = t.chipBg;
  el.style.color = t.chipText;
  el.style.border = `1px solid ${theme === "dark" ? "rgba(255,255,255,0.08)" : "rgba(0,0,0,0.06)"}`;
  el.textContent = text;
  return el;
}

// The classic MapLibre/Mapbox "animated dashed line" technique — cycling a
// sequence of dasharrays fakes a moving phase, since the style spec has no
// dash-offset paint property to animate directly.
const DASH_SEQUENCE: number[][] = [
  [0, 4, 3],
  [0.5, 4, 2.5],
  [1, 4, 2],
  [1.5, 4, 1.5],
  [2, 4, 1],
  [2.5, 4, 0.5],
  [3, 4, 0],
  [0, 0.5, 3, 3.5],
  [0, 1, 3, 3],
  [0, 1.5, 3, 2.5],
  [0, 2, 3, 2],
  [0, 2.5, 3, 1.5],
  [0, 3, 3, 1],
  [0, 3.5, 3, 0.5],
];

export interface MiniMapProps {
  scene: MiniMapScene | null;
  className?: string;
  /** Caption shown bottom-left before any scene is available. */
  emptyHint?: string;
}

/**
 * The signature small MapLibre view used on the docs home hero and every
 * endpoint's Try panel — a flat cartographic basemap (land/coast/district
 * lines from the real admin.pmtiles, NOT the CARTO raster used by the
 * flagship map), non-interactive, redrawn per MiniMapScene. Marker pulse and
 * the boundary's marching-ants dash both no-op under
 * prefers-reduced-motion.
 */
export function MiniMap({ scene, className, emptyHint }: MiniMapProps) {
  const { resolvedTheme } = useTheme();
  const mmTheme: MiniMapTheme = resolvedTheme;
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const mapRef = React.useRef<MaplibreMap | null>(null);
  const markersRef = React.useRef<Marker[]>([]);
  const dashRafRef = React.useRef<number | null>(null);
  const dashStepRef = React.useRef(0);
  const [mapReady, setMapReady] = React.useState(false);
  const [resolvedBoundary, setResolvedBoundary] = React.useState<GeoJSON.Feature | null>(null);
  const [pulsing, setPulsing] = React.useState(false);
  const reducedMotion = React.useMemo(reducedMotionQuery, []);

  // Construct once.
  React.useEffect(() => {
    const container = containerRef.current;
    if (!container || mapRef.current) return;

    const map = new MaplibreMap({
      container,
      style: baseStyle(mmTheme),
      center: [80.7, 7.5],
      zoom: 6.4,
      interactive: false,
      attributionControl: false,
    });

    const addOverlayLayers = () => {
      if (!map.getSource(SRC_BOUNDARY)) map.addSource(SRC_BOUNDARY, { type: "geojson", data: EMPTY_FC });
      if (!map.getSource(SRC_CIRCLE)) map.addSource(SRC_CIRCLE, { type: "geojson", data: EMPTY_FC });
      if (!map.getSource(SRC_DOTS)) map.addSource(SRC_DOTS, { type: "geojson", data: EMPTY_FC });
      if (!map.getSource(SRC_TILE)) map.addSource(SRC_TILE, { type: "geojson", data: EMPTY_FC });

      const t = MM_THEME[mmTheme];

      map.addLayer({ id: "mm-region-fill", type: "fill", source: SRC_BOUNDARY, paint: { "fill-color": t.accent, "fill-opacity": 0 } });
      map.addLayer({ id: "mm-boundary-fill", type: "fill", source: SRC_BOUNDARY, paint: { "fill-color": t.accent, "fill-opacity": 0.001 } });
      map.addLayer({
        id: "mm-boundary-glow",
        type: "line",
        source: SRC_BOUNDARY,
        layout: { "line-join": "round", "line-cap": "round" },
        paint: { "line-color": t.accent, "line-width": 7, "line-blur": 4, "line-opacity": 0.45 },
      });
      map.addLayer({
        id: "mm-boundary-line",
        type: "line",
        source: SRC_BOUNDARY,
        layout: { "line-join": "round", "line-cap": "round" },
        paint: { "line-color": t.accent, "line-width": 1.4, "line-opacity": 0.9 },
      });
      map.addLayer({
        id: "mm-boundary-dash",
        type: "line",
        source: SRC_BOUNDARY,
        layout: { "line-join": "round", "line-cap": "round" },
        paint: { "line-color": "#ffffff", "line-width": 1.2, "line-opacity": 0.85, "line-dasharray": DASH_SEQUENCE[0] },
      });
      map.addLayer({
        id: "mm-circle-line",
        type: "line",
        source: SRC_CIRCLE,
        paint: { "line-color": t.accent, "line-width": 1.2, "line-opacity": 0.7, "line-dasharray": [3, 2.5] },
      });
      map.addLayer({
        id: "mm-dots",
        type: "circle",
        source: SRC_DOTS,
        paint: {
          "circle-color": ["get", "color"],
          "circle-radius": ["case", ["boolean", ["get", "grid"], false], 2.1, 3.4],
          "circle-opacity": 0.88,
          "circle-stroke-width": 0,
        },
      });
      map.addLayer({ id: "mm-tile-fill", type: "fill", source: SRC_TILE, paint: { "fill-color": t.accent, "fill-opacity": 0.08 } });
      map.addLayer({
        id: "mm-tile-line",
        type: "line",
        source: SRC_TILE,
        paint: { "line-color": t.accent, "line-width": 1.2, "line-dasharray": [2, 1.5], "line-opacity": 0.85 },
      });
    };

    if (map.isStyleLoaded()) addOverlayLayers();
    else map.once("style.load", addOverlayLayers);

    mapRef.current = map;
    setMapReady(true);

    return () => {
      if (dashRafRef.current !== null) cancelAnimationFrame(dashRafRef.current);
      for (const m of markersRef.current) m.remove();
      markersRef.current = [];
      map.remove();
      mapRef.current = null;
      setMapReady(false);
    };
    // Constructed once; theme changes are handled by the effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // March the boundary dash overlay (skipped entirely under reduced motion — a static dasharray is set once above instead).
  React.useEffect(() => {
    if (reducedMotion) return;
    const map = mapRef.current;
    if (!map || !mapReady) return;

    function tick(timestamp: number) {
      const step = Math.floor((timestamp / 60) % DASH_SEQUENCE.length);
      if (step !== dashStepRef.current) {
        dashStepRef.current = step;
        if (mapRef.current?.getLayer("mm-boundary-dash")) {
          mapRef.current.setPaintProperty("mm-boundary-dash", "line-dasharray", DASH_SEQUENCE[step]);
        }
      }
      dashRafRef.current = requestAnimationFrame(tick);
    }
    dashRafRef.current = requestAnimationFrame(tick);
    return () => {
      if (dashRafRef.current !== null) cancelAnimationFrame(dashRafRef.current);
    };
  }, [mapReady, reducedMotion]);

  // Recolor the base style + overlay layer accents when the resolved theme flips.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const t = MM_THEME[mmTheme];
    const apply = () => {
      map.setPaintProperty("mm-bg", "background-color", t.water);
      map.setPaintProperty("mm-land-fill", "fill-color", t.land);
      map.setPaintProperty("mm-district-line", "line-color", t.line);
      map.setPaintProperty("mm-coast-line", "line-color", t.coast);
      for (const id of ["mm-boundary-fill", "mm-region-fill", "mm-boundary-glow", "mm-boundary-line", "mm-circle-line", "mm-tile-fill", "mm-tile-line"]) {
        if (map.getLayer(id)) {
          const prop = id.includes("fill") ? "fill-color" : "line-color";
          map.setPaintProperty(id, prop as never, t.accent as never);
        }
      }
    };
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [mmTheme, mapReady]);

  // Resolve a boundary that has to be fetched (reverse/admin views only carry a pcode, not the polygon).
  React.useEffect(() => {
    setResolvedBoundary(null);
    if (!scene?.boundaryPcode) return;
    const controller = new AbortController();
    apiGetRaw<GeoJSON.Feature>(`/admin/${encodeURIComponent(scene.boundaryPcode)}/geometry`, { signal: controller.signal })
      .then((feature) => setResolvedBoundary(feature))
      .catch(() => setResolvedBoundary(null));
    return () => controller.abort();
  }, [scene?.boundaryPcode]);

  // Paint the current scene: camera, boundary, circle, ramp dots, markers, chips, tile rect, party fill.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;

    const apply = () => {
      const t = MM_THEME[mmTheme];

      for (const m of markersRef.current) m.remove();
      markersRef.current = [];

      const boundarySource = map.getSource(SRC_BOUNDARY) as GeoJSONSource | undefined;
      const circleSource = map.getSource(SRC_CIRCLE) as GeoJSONSource | undefined;
      const dotsSource = map.getSource(SRC_DOTS) as GeoJSONSource | undefined;
      const tileSource = map.getSource(SRC_TILE) as GeoJSONSource | undefined;

      if (!scene) {
        boundarySource?.setData(EMPTY_FC);
        circleSource?.setData(EMPTY_FC);
        dotsSource?.setData(EMPTY_FC);
        tileSource?.setData(EMPTY_FC);
        map.easeTo({ center: [80.7, 7.5], zoom: 6.4, duration: 500 });
        return;
      }

      const boundaryFeature = scene.boundary ?? resolvedBoundary;
      const isElections = scene.view === "elections" && scene.fillColor;
      if (isElections) {
        const region = circleFeature(scene.center[1], scene.center[0], REGION_FILL_RADIUS_KM);
        boundarySource?.setData({ type: "FeatureCollection", features: [region] });
        if (map.getLayer("mm-region-fill")) map.setPaintProperty("mm-region-fill", "fill-color", scene.fillColor!);
        if (map.getLayer("mm-region-fill")) map.setPaintProperty("mm-region-fill", "fill-opacity", 0.28);
        if (map.getLayer("mm-boundary-line")) map.setPaintProperty("mm-boundary-line", "line-color", scene.fillColor!);
        for (const id of ["mm-boundary-fill", "mm-boundary-glow", "mm-boundary-dash"]) {
          if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", "none");
        }
        if (map.getLayer("mm-boundary-line")) map.setLayoutProperty("mm-boundary-line", "visibility", "visible");
      } else if (boundaryFeature) {
        boundarySource?.setData({ type: "FeatureCollection", features: [boundaryFeature] });
        for (const id of ["mm-boundary-fill", "mm-boundary-glow", "mm-boundary-line", "mm-boundary-dash"]) {
          if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", "visible");
        }
        // The design's "glowing boundary" is a soft tinted fill plus a blurred accent stroke — glowFill already
        // carries its own alpha (rgba), so the fill layer's own fill-opacity paint stays at 1.
        if (map.getLayer("mm-boundary-fill")) {
          map.setPaintProperty("mm-boundary-fill", "fill-color", t.glowFill);
          map.setPaintProperty("mm-boundary-fill", "fill-opacity", 1);
        }
        if (map.getLayer("mm-boundary-line")) map.setPaintProperty("mm-boundary-line", "line-color", t.accent);
        if (map.getLayer("mm-region-fill")) map.setLayoutProperty("mm-region-fill", "visibility", "none");
        if (reducedMotion && map.getLayer("mm-boundary-dash")) map.setLayoutProperty("mm-boundary-dash", "visibility", "none");
      } else {
        boundarySource?.setData(EMPTY_FC);
        for (const id of ["mm-boundary-fill", "mm-boundary-glow", "mm-boundary-line", "mm-boundary-dash", "mm-region-fill"]) {
          if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", "none");
        }
      }

      if (scene.circle) {
        const feature = circleFeature(scene.circle.lat, scene.circle.lon, scene.circle.radiusKm);
        circleSource?.setData({ type: "FeatureCollection", features: [feature] });
        if (map.getLayer("mm-circle-line")) {
          map.setLayoutProperty("mm-circle-line", "visibility", "visible");
          map.setPaintProperty("mm-circle-line", "line-dasharray", scene.circle.dashed ? [3, 2.5] : [1, 0]);
        }
      } else {
        circleSource?.setData(EMPTY_FC);
        if (map.getLayer("mm-circle-line")) map.setLayoutProperty("mm-circle-line", "visibility", "none");
      }

      if (scene.rampDots.length > 0) {
        const features: GeoJSON.Feature[] = scene.rampDots.map((d) => ({
          type: "Feature",
          properties: { color: populationRampColor(d.t), grid: !!scene.isGrid },
          geometry: { type: "Point", coordinates: [d.lon, d.lat] },
        }));
        dotsSource?.setData({ type: "FeatureCollection", features });
      } else {
        dotsSource?.setData(EMPTY_FC);
      }

      if (scene.tileRect) {
        tileSource?.setData({ type: "FeatureCollection", features: [scene.tileRect] });
      } else {
        tileSource?.setData(EMPTY_FC);
      }

      for (const marker of scene.markers) {
        const el = buildMarkerElement({ primary: !!marker.primary, color: t.marker, reducedMotion });
        const m = new Marker({ element: el, anchor: "center" }).setLngLat([marker.lon, marker.lat]).addTo(map);
        markersRef.current.push(m);
      }
      for (const chip of scene.chips) {
        const el = buildChipElement(chip.text, mmTheme);
        const m = new Marker({ element: el, anchor: "bottom", offset: [0, -10] }).setLngLat([chip.lon, chip.lat]).addTo(map);
        markersRef.current.push(m);
      }

      map.easeTo({ center: scene.center, zoom: scene.zoom, duration: 700 });
    };

    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, resolvedBoundary, mapReady, mmTheme, reducedMotion]);

  // "On send" pulse: a brief dim + fade back, matching the design's send-triggered refresh cue.
  React.useEffect(() => {
    if (!scene) return;
    setPulsing(true);
    const timer = window.setTimeout(() => setPulsing(false), 220);
    return () => window.clearTimeout(timer);
  }, [scene]);

  return (
    <div className={cn("relative overflow-hidden", className)}>
      <div
        ref={containerRef}
        className={cn("size-full transition-opacity duration-300", pulsing && !reducedMotion ? "opacity-30" : "opacity-100")}
      />
      {!scene && emptyHint && (
        <div className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center">
          <span className="rounded-full bg-bg2/85 px-2.5 py-1 font-mono text-[10px] text-ink3">{emptyHint}</span>
        </div>
      )}
      <div className="pointer-events-none absolute bottom-1 right-2 font-mono text-[8.5px] text-ink3/80">geoBoundaries · OCHA · OSM</div>
    </div>
  );
}
