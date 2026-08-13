import * as React from "react";
import {
  AttributionControl,
  Map as MaplibreMap,
  Marker,
  NavigationControl,
  ScaleControl,
} from "maplibre-gl";
import type { GeoJSONSource, IControl, LayerSpecification, LngLatBoundsLike, MapMouseEvent } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { MapboxOverlay } from "@deck.gl/mapbox";

import { useTheme } from "@/components/theme-provider";
import { applyBasemapMode, basemapStyle, resolveBasemapMode, type BasemapMode } from "@/components/map/basemap";
import {
  ADMIN_HOVER_LAYER_IDS,
  ADMIN_SOURCE_ID,
  applyThemedPaint,
  LAYER_REGISTRY,
} from "@/components/map/layer-registry";
import { buildHighlightMarkerElement } from "@/components/map/highlight-marker";
import { LayerPanel } from "@/components/map/layer-panel";
import { MapLegend } from "@/components/map/legend";
import { buildPopulationLayer, POPULATION_LAYER_ID, populationTooltip } from "@/components/map/population-layer";
import "@/components/map/pmtiles-protocol";
import { ExploreAttribution } from "@/components/explore/explore-attribution";
import { ExploreHeader } from "@/components/explore/explore-header";
import { ExploreOmnibox } from "@/components/explore/explore-omnibox";
import { ExploreReadout } from "@/components/explore/explore-readout";
import { glassPanelStyle, MONO_FONT, SANS_FONT, TEXT, TEXT2 } from "@/components/explore/glass";
import { featureBounds } from "@/lib/geojson-bounds";
import { usePopulationGrid } from "@/hooks/use-population-grid";
import { useLayerStore } from "@/stores/layer-store";
import { useMapStore, DEFAULT_MAP_VIEW } from "@/stores/map-store";

// Above this zoom the ~1.1 km density columns are wider than the viewport;
// the population-3d layer hides itself and returns when the user zooms out.
const POPULATION_MAX_ZOOM = 11.5;

// The island is the product: the camera is locked to Sri Lanka's own
// bounding box (from the geoBoundaries outline that also feeds
// public/country-mask.json), padded just enough that the coast isn't
// glued to the panel edges.
const COUNTRY_BBOX: LngLatBoundsLike = [
  [79.6509, 5.919],
  [81.879, 9.8358],
];
// Generous on purpose: maxBounds doubles as a zoom clamp — MapLibre will
// force-zoom IN until the viewport fits inside the bounds, so a tight cage
// silently overrides fitBounds on wide viewports (island height on screen
// needs lots of longitude at 21:9). ±5°/±3° keeps panning loosely tethered
// to the island while never fighting the fit; the sea mask makes the extra
// area visually inert anyway.
const CAMERA_BOUNDS: LngLatBoundsLike = [
  [79.6509 - 5, 5.919 - 3],
  [81.879 + 5, 9.8358 + 3],
];
// Mirrors the design mock's fitExtent margins: clear of the header/layer
// panel on the left and the omnibox strip on top.
const FIT_PADDING = { top: 76, bottom: 56, left: 270, right: 64 };

// Everything that isn't Sri Lanka is painted over with a flat sea fill —
// a world polygon with the country outline as holes (web/public/country-mask.json).
const MASK_SOURCE_ID = "country-mask";
const MASK_LAYER_ID = "country-mask-fill";
const MASK_SEA: Record<BasemapMode, string> = { dark: "#07090C", light: "#EAEFF3" };

const POPULATION_PITCH = 50;

const HIGHLIGHT_SOURCE_ID = "highlight";
const HIGHLIGHT_FILL_LAYER_ID = "highlight-fill";
const HIGHLIGHT_GLOW_LAYER_ID = "highlight-line-glow";
const HIGHLIGHT_LINE_LAYER_ID = "highlight-line";
const HIGHLIGHT_LAYER_IDS = [HIGHLIGHT_FILL_LAYER_ID, HIGHLIGHT_GLOW_LAYER_ID, HIGHLIGHT_LINE_LAYER_ID];

const EMPTY_FEATURE_COLLECTION: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

// Highlight accent — the Explore design's accent token (dark #D8446B,
// light #8D153A per DESIGN-NOTES.md's token table), as a plain hex pair.
// MapLibre's paint expressions parse CSS colors themselves and don't
// resolve `var(--...)` custom properties, so this can't just reference
// --accent; it's kept in sync by eye instead.
const HIGHLIGHT_ACCENT: Record<BasemapMode, string> = { light: "#8D153A", dark: "#D8446B" };

/** Re-applies the highlight layers' accent color for `mode`. Paired with applyBasemapMode/applyThemedPaint in the theme-change effect — highlight layers live outside LAYER_REGISTRY so they need their own call. */
function applyHighlightTheme(map: MaplibreMap, mode: BasemapMode): void {
  const accent = HIGHLIGHT_ACCENT[mode];
  if (map.getLayer(HIGHLIGHT_FILL_LAYER_ID)) map.setPaintProperty(HIGHLIGHT_FILL_LAYER_ID, "fill-color", accent);
  if (map.getLayer(HIGHLIGHT_GLOW_LAYER_ID)) map.setPaintProperty(HIGHLIGHT_GLOW_LAYER_ID, "line-color", accent);
  if (map.getLayer(HIGHLIGHT_LINE_LAYER_ID)) map.setPaintProperty(HIGHLIGHT_LINE_LAYER_ID, "line-color", accent);
}

interface HoveredAdmin {
  pcode: string;
  name: string;
}

/** Tracks the currently feature-stated ("hover": true) admin feature so it can be cleared before the next one is set, or on mouseout. */
interface HoverRef {
  sourceLayer: string;
  id: string | number;
}

/**
 * The MapLibre canvas plus its deck.gl overlay. Base layers come entirely
 * from LAYER_REGISTRY — MapView just walks that array for the MapLibre
 * (`kind: "maplibre"`) entries and adds/removes/toggles them; the one
 * `kind: "deck"` entry (population-3d) is built imperatively here from
 * population-layer.ts and pushed into the MapboxOverlay.
 *
 * The map is the source of truth for camera position once mounted; it only
 * reads the store once, at construction, for the initial view (already
 * restored from the URL by useMapUrlSync before this mounts). After that it
 * pushes moveend updates back into the store one-way, EXCEPT for two
 * store -> map paths added for this feature: `flyToRequest` (generic camera
 * moves other features can request) and `highlight` (auto-fit/flyTo to
 * whatever's highlighted).
 */
export function MapView() {
  const { theme } = useTheme();
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const mapRef = React.useRef<MaplibreMap | null>(null);
  const overlayRef = React.useRef<MapboxOverlay | null>(null);
  const markerRef = React.useRef<Marker | null>(null);
  const hoverRef = React.useRef<HoverRef | null>(null);
  const populationPitchedRef = React.useRef(false);

  const visibility = useLayerStore((s) => s.visibility);
  const highlight = useMapStore((s) => s.highlight);
  const flyToRequest = useMapStore((s) => s.flyToRequest);

  const [hoveredAdmin, setHoveredAdmin] = React.useState<HoveredAdmin | null>(null);
  // Bottom-left coordinate readout — updated on every mousemove; falls back
  // to the map's initial center (read once, not subscribed reactively —
  // see the class doc comment above on why this component avoids
  // store->map reactive coupling outside flyToRequest/highlight) until the
  // pointer first moves over the map.
  const [pointer, setPointer] = React.useState<{ lat: number; lon: number } | null>(null);
  const initialCenterRef = React.useRef(useMapStore.getState().center);
  // Flips true once createMap() finishes (see the construction effect
  // below). mapRef/overlayRef are refs, not state, precisely so the other
  // effects can read them without re-running on every map-internal change
  // — but that means an effect that fires *before* the map exists (a very
  // real race: map construction is deferred behind a ResizeObserver, see
  // below) reads a null ref, no-ops, and — since refs aren't reactive —
  // never gets another chance once the map shows up. mapReady in each
  // effect's dependency array is what forces that one retry.
  const [mapReady, setMapReady] = React.useState(false);
  const [mapZoom, setMapZoom] = React.useState(DEFAULT_MAP_VIEW.zoom);

  const populationVisible = visibility[POPULATION_LAYER_ID] ?? false;
  const { data: populationGrid } = usePopulationGrid(populationVisible);
  // The density buckets are ~1.1 km wide (res 0.02): past this zoom each
  // column fills the viewport and buries whatever the user zoomed in on
  // (e.g. a highlighted GN division), so the layer bows out.
  const populationInZoomRange = mapZoom < POPULATION_MAX_ZOOM;

  // ---------------------------------------------------------------------
  // Map + deck.gl overlay construction (once).
  // ---------------------------------------------------------------------
  React.useEffect(() => {
    const container = containerRef.current;
    if (!container || mapRef.current) return;

    let map: MaplibreMap | null = null;
    let detach: (() => void) | null = null;

    function createMap(): void {
      if (map) return;
      const mode = resolveBasemapMode(theme);

      const nextMap = new MaplibreMap({
        container: container as HTMLDivElement,
        style: basemapStyle(mode),
        center: [80.77, 7.87],
        zoom: 7,
        minZoom: 5,
        maxZoom: 18,
        maxBounds: CAMERA_BOUNDS,
        attributionControl: false,
      });

      // Fit-to-island: the map always OPENS with the whole country framed
      // for the current viewport (the fit is computed per viewport size, so
      // a phone and an ultrawide both get the full island). The zoom floor
      // sits ZOOM_OUT_HEADROOM below the fit, so pulling back for context
      // is possible but the island can never be lost off-screen (maxBounds
      // still pins the camera to it).
      //
      // The fit is recomputed — and RE-APPLIED — on every resize and once on
      // "load" until the user's first gesture. Construction can happen while
      // the container is still settling into its final layout size, so the
      // first fit may frame the island for the wrong viewport; only a real
      // interaction (drag/wheel/touch/control click) freezes the camera as
      // the user's own.
      const ZOOM_OUT_HEADROOM = 1.4;
      let userInteracted = false;
      const markInteracted = () => {
        userInteracted = true;
      };
      container?.addEventListener("pointerdown", markInteracted, { capture: true });
      container?.addEventListener("wheel", markInteracted, { capture: true, passive: true });
      const applyIslandFit = (jump: boolean) => {
        const cam = nextMap.cameraForBounds(COUNTRY_BBOX, { padding: FIT_PADDING });
        if (!cam) return;
        nextMap.setMinZoom(Math.max(4.5, (cam.zoom ?? 6.5) - ZOOM_OUT_HEADROOM));
        if (jump) nextMap.jumpTo({ center: cam.center, zoom: cam.zoom, pitch: 0, bearing: 0 });
      };
      if (import.meta.env.DEV) (window as unknown as Record<string, unknown>).__map = nextMap;
      const fitUnlessInteracted = () => applyIslandFit(!userInteracted);
      applyIslandFit(true);
      // The constructor can measure the container mid-layout, leaving the
      // transform (and therefore cameraForBounds) with stale dimensions —
      // and "load" is no rescue when tiles fail. One frame later layout has
      // settled: force a re-measure, then fit again.
      requestAnimationFrame(() => {
        nextMap.resize();
        fitUnlessInteracted();
      });
      nextMap.once("load", fitUnlessInteracted);
      nextMap.on("resize", fitUnlessInteracted);

      // Sea mask: world polygon with the country as holes, painted the
      // design's flat sea color so neighboring coastlines and ocean labels
      // from the raster basemap never show.
      fetch("/country-mask.json")
        .then((r) => (r.ok ? r.json() : null))
        .then((mask: GeoJSON.Feature | null) => {
          if (!mask || nextMap.getSource(MASK_SOURCE_ID)) return;
          const addMask = () => {
            if (nextMap.getSource(MASK_SOURCE_ID)) return;
            nextMap.addSource(MASK_SOURCE_ID, { type: "geojson", data: mask });
            // Insert below every vector layer but above the raster basemap:
            // the first non-raster layer is the right anchor.
            const anchor = nextMap.getStyle().layers?.find((l) => l.type !== "raster")?.id;
            nextMap.addLayer(
              {
                id: MASK_LAYER_ID,
                type: "fill",
                source: MASK_SOURCE_ID,
                paint: { "fill-color": MASK_SEA[resolveBasemapMode(theme)], "fill-opacity": 1 },
              },
              anchor,
            );
          };
          // Not isStyleLoaded(): that stays false while raster tiles are
          // still fetching, long after the style itself (an inline object)
          // is usable — and "style.load" has already fired by then, so
          // waiting for it again would wait forever. The style object being
          // present is the actual precondition for addLayer.
          if (nextMap.getStyle()) addMask();
          else nextMap.once("styledata", addMask);
        })
        .catch(() => undefined);

      nextMap.addControl(new AttributionControl({ compact: true }));
      nextMap.addControl(new NavigationControl({ showCompass: true }), "top-right");
      nextMap.addControl(new ScaleControl({ maxWidth: 120, unit: "metric" }), "bottom-left");

      // interleaved: false (overlaid mode), deliberately deviating from the
      // task brief's `interleaved: true`. Verified (not assumed) that
      // interleaved mode is broken with this exact dependency pair —
      // @deck.gl/mapbox@9.3.10 against maplibre-gl@6.3.0 (a very recent
      // major release): the data pipeline, WebGL2 context, and the
      // MapboxLayerGroup custom layer's position in the style's layer
      // order were all confirmed correct, and picking correctly resolved
      // screen positions to the right coordinates — but nothing actually
      // rasterized to the framebuffer (population-3d never painted a
      // pixel, at any zoom/pitch). Switching to overlaid mode fixed
      // rendering immediately with no other changes, isolating the fault
      // to interleaved compositing. Overlaid mode still shares the same
      // camera sync, hover-picking (via forwarded mouse events), and
      // tooltip behavior — the only real difference is that its layers
      // can't be interleaved *between* MapLibre layers (always draws
      // above all of them), which doesn't matter here: population-3d is
      // meant to sit above everything else anyway.
      const overlay = new MapboxOverlay({
        interleaved: false,
        layers: [],
        getTooltip: populationTooltip,
      });
      nextMap.addControl(overlay as unknown as IControl);

      const onMoveEnd = () => {
        const c = nextMap.getCenter();
        useMapStore.getState().setView({ center: [c.lng, c.lat], zoom: nextMap.getZoom() });
      };
      nextMap.on("moveend", onMoveEnd);

      const onZoom = () => setMapZoom(nextMap.getZoom());
      nextMap.on("zoom", onZoom);

      const onClick = () => useMapStore.getState().setHighlight(null);
      nextMap.on("click", onClick);

      const clearHover = () => {
        if (hoverRef.current) {
          nextMap.setFeatureState(
            { source: ADMIN_SOURCE_ID, sourceLayer: hoverRef.current.sourceLayer, id: hoverRef.current.id },
            { hover: false }
          );
          hoverRef.current = null;
        }
        setHoveredAdmin(null);
        nextMap.getCanvas().style.cursor = "";
      };

      const onMouseMove = (e: MapMouseEvent) => {
        setPointer({ lat: e.lngLat.lat, lon: e.lngLat.lng });
        const layerIds = ADMIN_HOVER_LAYER_IDS.filter((id) => nextMap.getLayer(id));
        if (layerIds.length === 0) return;
        const features = nextMap.queryRenderedFeatures(e.point, { layers: layerIds });
        const top = features[0];
        if (!top || top.id === undefined || !top.sourceLayer) {
          clearHover();
          return;
        }
        if (hoverRef.current && (hoverRef.current.sourceLayer !== top.sourceLayer || hoverRef.current.id !== top.id)) {
          nextMap.setFeatureState(
            { source: ADMIN_SOURCE_ID, sourceLayer: hoverRef.current.sourceLayer, id: hoverRef.current.id },
            { hover: false }
          );
        }
        nextMap.setFeatureState({ source: ADMIN_SOURCE_ID, sourceLayer: top.sourceLayer, id: top.id }, { hover: true });
        hoverRef.current = { sourceLayer: top.sourceLayer, id: top.id };
        setHoveredAdmin({
          pcode: String(top.properties?.pcode ?? ""),
          name: String(top.properties?.name_en ?? ""),
        });
        nextMap.getCanvas().style.cursor = "pointer";
      };
      nextMap.on("mousemove", onMouseMove);
      nextMap.on("mouseout", clearHover);

      const addRegistryLayers = () => {
        // Theme at mount time; the theme-change effect below keeps this in
        // sync afterward via applyThemedPaint (paired with applyBasemapMode).
        const mode = resolveBasemapMode(theme);
        for (const layer of LAYER_REGISTRY) {
          if (layer.kind !== "maplibre") continue;
          if (!nextMap.getSource(layer.id)) {
            nextMap.addSource(layer.id, layer.source);
          }
          const themed = layer.themedPaint?.(mode) ?? {};
          for (const spec of layer.layers) {
            if (nextMap.getLayer(spec.id)) continue;
            const overrides = themed[spec.id];
            // Merging in themedPaint's dynamically-keyed overrides breaks
            // LayerSpecification's discriminated-union narrowing on `type`;
            // the merged object is still a valid layer spec (overrides are
            // just paint props from this same layer's own themedPaint fn).
            const merged = overrides ? ({ ...spec, paint: { ...spec.paint, ...overrides } } as LayerSpecification) : spec;
            nextMap.addLayer(merged);
          }
        }

        // Highlight source/layers, added last so they draw above every
        // registry layer (glowing outline + soft fill, theme-aware accent).
        const accent = HIGHLIGHT_ACCENT[mode];
        if (!nextMap.getSource(HIGHLIGHT_SOURCE_ID)) {
          nextMap.addSource(HIGHLIGHT_SOURCE_ID, { type: "geojson", data: EMPTY_FEATURE_COLLECTION });
        }
        if (!nextMap.getLayer(HIGHLIGHT_FILL_LAYER_ID)) {
          nextMap.addLayer({
            id: HIGHLIGHT_FILL_LAYER_ID,
            type: "fill",
            source: HIGHLIGHT_SOURCE_ID,
            layout: { visibility: "none" },
            paint: { "fill-color": accent, "fill-opacity": 0.12 },
          });
        }
        if (!nextMap.getLayer(HIGHLIGHT_GLOW_LAYER_ID)) {
          nextMap.addLayer({
            id: HIGHLIGHT_GLOW_LAYER_ID,
            type: "line",
            source: HIGHLIGHT_SOURCE_ID,
            layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
            paint: {
              "line-color": accent,
              "line-width": 9,
              "line-blur": 4,
              "line-opacity": 0.45,
            },
          });
        }
        if (!nextMap.getLayer(HIGHLIGHT_LINE_LAYER_ID)) {
          nextMap.addLayer({
            id: HIGHLIGHT_LINE_LAYER_ID,
            type: "line",
            source: HIGHLIGHT_SOURCE_ID,
            layout: { visibility: "none", "line-join": "round", "line-cap": "round" },
            paint: { "line-color": accent, "line-width": 2.5, "line-opacity": 0.95 },
          });
        }
      };
      if (nextMap.isStyleLoaded()) addRegistryLayers();
      else nextMap.once("style.load", addRegistryLayers);

      map = nextMap;
      mapRef.current = nextMap;
      overlayRef.current = overlay;
      setMapReady(true);

      detach = () => {
        nextMap.off("moveend", onMoveEnd);
        nextMap.off("click", onClick);
        nextMap.off("mousemove", onMouseMove);
        nextMap.off("mouseout", clearHover);
        markerRef.current?.remove();
        markerRef.current = null;
        overlay.finalize();
        overlayRef.current = null;
        nextMap.remove();
        mapRef.current = null;
        setMapReady(false);
      };
    }

    // Don't construct the map until `container` actually has a non-zero
    // size. On a cold dev-server load, Tailwind's stylesheet can still be
    // in flight when this effect first runs (Vite serves it as a separate
    // request), so the container can genuinely measure zero-height at this
    // exact moment; MapLibre sizes its WebGL canvas once, synchronously,
    // from whatever it measures at construction time, and its own internal
    // ResizeObserver doesn't reliably recover from a bad *initial* size in
    // that scenario (initialization partially fails instead — the canvas
    // gets stuck at a fallback size and never receives tiles). Gating
    // construction on our own ResizeObserver's first real measurement
    // sidesteps the race instead of papering over its symptom. Once
    // created, the same observer keeps calling `resize()` on later size
    // changes (sidebar/viewport), same as MapLibre's own would.
    const resizeObserver = new ResizeObserver(() => {
      if (map) {
        map.resize();
        return;
      }
      const rect = container.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      createMap();
    });
    resizeObserver.observe(container);
    // Also measure synchronously: per spec ResizeObserver fires once on
    // observe(), but that initial delivery cannot be relied on in every
    // embedding (and even when it comes, it's a task later). If the
    // container is already laid out — the common case — construct now; the
    // observer then only handles genuine size changes and the zero-size
    // cold-load race documented above.
    {
      const rect = container.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) createMap();
    }

    return () => {
      resizeObserver.disconnect();
      detach?.();
    };
    // Map is created once on mount; theme is handled by the effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Swap the basemap tiles + every registry layer's themed paint props when
  // the resolved theme changes.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const mode = resolveBasemapMode(theme);
    const apply = () => {
      applyBasemapMode(map, mode);
      applyThemedPaint(map, mode);
      applyHighlightTheme(map, mode);
      if (map.getLayer(MASK_LAYER_ID)) map.setPaintProperty(MASK_LAYER_ID, "fill-color", MASK_SEA[mode]);
    };
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [theme, mapReady]);

  // Show/hide registry layers as the layer store changes.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const apply = () => {
      for (const layer of LAYER_REGISTRY) {
        if (layer.kind !== "maplibre") continue;
        const value = visibility[layer.id] ? "visible" : "none";
        for (const spec of layer.layers) {
          if (map.getLayer(spec.id)) {
            map.setLayoutProperty(spec.id, "visibility", value);
          }
        }
      }
    };
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [visibility, mapReady]);

  // population-3d: tilt into a 3D reveal when toggled on, level back out
  // when toggled off.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    if (populationVisible === populationPitchedRef.current) return;
    populationPitchedRef.current = populationVisible;
    const apply = () => map.easeTo({ pitch: populationVisible ? POPULATION_PITCH : 0, duration: 800 });
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [populationVisible, mapReady]);

  // Rebuild the population-3d deck.gl layers whenever visibility or data
  // changes. The ramp is theme-invariant (see population-layer.ts), so
  // unlike the other registry layers this doesn't need to react to theme.
  React.useEffect(() => {
    const overlay = overlayRef.current;
    if (!overlay) return;
    overlay.setProps({
      layers: buildPopulationLayer({
        cells: populationGrid?.cells ?? [],
        visible: populationVisible && populationInZoomRange,
      }),
    });
  }, [populationGrid, populationVisible, populationInZoomRange, mapReady]);

  // Generic camera moves requested via mapStore.flyTo (other features).
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map || !flyToRequest) return;
    const apply = () =>
      map.flyTo({
        center: [flyToRequest.lon, flyToRequest.lat],
        zoom: flyToRequest.zoom ?? map.getZoom(),
        essential: true,
        duration: 1000,
      });
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [flyToRequest, mapReady]);

  // Highlight: draw the geometry outline/fill or the point marker, and
  // auto-fit/flyTo the target. Clears everything when highlight is null.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;

    const apply = () => {
      markerRef.current?.remove();
      markerRef.current = null;

      const source = map.getSource(HIGHLIGHT_SOURCE_ID) as GeoJSONSource | undefined;

      if (highlight?.kind === "geometry") {
        source?.setData({ type: "FeatureCollection", features: [highlight.feature] });
        for (const id of HIGHLIGHT_LAYER_IDS) {
          if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", "visible");
        }
        const bounds = featureBounds(highlight.feature);
        if (bounds) {
          map.fitBounds(bounds, { padding: 64, maxZoom: 14, duration: 800 });
        }
      } else {
        source?.setData(EMPTY_FEATURE_COLLECTION);
        for (const id of HIGHLIGHT_LAYER_IDS) {
          if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", "none");
        }
      }

      if (highlight?.kind === "point") {
        const el = buildHighlightMarkerElement(highlight.label);
        const marker = new Marker({ element: el, anchor: "center" })
          .setLngLat([highlight.lon, highlight.lat])
          .addTo(map);
        markerRef.current = marker;
        map.flyTo({ center: [highlight.lon, highlight.lat], zoom: 13, essential: true, duration: 800 });
      }
    };

    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [highlight, mapReady]);

  const mode = resolveBasemapMode(theme);
  const readoutLat = pointer?.lat ?? initialCenterRef.current[1];
  const readoutLon = pointer?.lon ?? initialCenterRef.current[0];

  return (
    <div className="relative size-full">
      {/*
        size-full, not absolute inset-0: MapLibre sets `position: relative`
        as an inline style on its container once it mounts, which outranks
        this className and makes `inset-0` a no-op (inset only sizes
        absolute/fixed elements) — the container would collapse to its
        content's height, which at that point is nothing, i.e. 0. Percentage
        sizing works under any position value, so it survives the override.
      */}
      <div ref={containerRef} className="size-full" />
      <ExploreHeader />
      <ExploreOmnibox />
      <LayerPanel />
      <MapLegend />
      <ExploreReadout lat={readoutLat} lon={readoutLon} zoom={mapZoom} />
      <ExploreAttribution />
      {hoveredAdmin && hoveredAdmin.name && (
        <div className="pointer-events-none absolute right-3 top-20 z-20" style={{ fontFamily: SANS_FONT }}>
          <div className="rounded-xl border px-3 py-2" style={glassPanelStyle(mode)}>
            <div className="text-sm font-medium" style={{ color: TEXT }}>
              {hoveredAdmin.name}
            </div>
            <div className="text-xs" style={{ color: TEXT2, fontFamily: MONO_FONT }}>
              {hoveredAdmin.pcode}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
