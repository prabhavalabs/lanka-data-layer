import * as React from "react";
import { AttributionControl, Map as MaplibreMap, Marker, NavigationControl } from "maplibre-gl";
import type { GeoJSONSource } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";

import { useTheme } from "@/components/theme-provider";
import { applyBasemapMode, basemapStyle, resolveBasemapMode, type BasemapMode } from "@/components/map/basemap";
import { buildHighlightMarkerElement } from "@/components/map/highlight-marker";
import "@/components/map/pmtiles-protocol";
import { featureBounds } from "@/lib/geojson-bounds";

const DEFAULT_CENTER: [number, number] = [80.7, 7.5];
const DEFAULT_ZOOM = 7.2;

const BOUNDARY_SOURCE_ID = "demo-boundary";
const BOUNDARY_FILL_LAYER_ID = "demo-boundary-fill";
const BOUNDARY_GLOW_LAYER_ID = "demo-boundary-glow";
const BOUNDARY_LINE_LAYER_ID = "demo-boundary-line";

const ACCENT: Record<BasemapMode, string> = { light: "#0369a1", dark: "#38bdf8" };

const EMPTY_FEATURE_COLLECTION: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

export interface DemoMapPoint {
  lat: number;
  lon: number;
  label?: string;
}

/**
 * Small, self-contained MapLibre instance for the /docs/demo postal-code
 * showcase — deliberately NOT MapView or stores/map-store.ts (per the docs
 * agent's scope: this page owns its own map, not the app's shared one). It
 * reuses the same basemap.ts style + pmtiles-protocol.ts registration and
 * highlight-marker.ts marker element as the real map, for visual
 * consistency, but keeps its own camera and its own GeoJSON source for the
 * boundary glow (styled the same way MapView's highlight layers are).
 */
export function DemoMap({ point, geometry }: { point: DemoMapPoint | null; geometry: GeoJSON.Feature | null }) {
  const { theme } = useTheme();
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const mapRef = React.useRef<MaplibreMap | null>(null);
  const markerRef = React.useRef<Marker | null>(null);
  const [mapReady, setMapReady] = React.useState(false);

  // Construct once.
  React.useEffect(() => {
    const container = containerRef.current;
    if (!container || mapRef.current) return;

    const mode = resolveBasemapMode(theme);
    const map = new MaplibreMap({
      container,
      style: basemapStyle(mode),
      center: DEFAULT_CENTER,
      zoom: DEFAULT_ZOOM,
      minZoom: 5,
      maxZoom: 16,
      attributionControl: false,
    });
    map.addControl(new AttributionControl({ compact: true }));
    map.addControl(new NavigationControl({ showCompass: false }), "top-right");

    const addBoundaryLayers = () => {
      const accent = ACCENT[resolveBasemapMode(theme)];
      if (!map.getSource(BOUNDARY_SOURCE_ID)) {
        map.addSource(BOUNDARY_SOURCE_ID, { type: "geojson", data: EMPTY_FEATURE_COLLECTION });
      }
      if (!map.getLayer(BOUNDARY_FILL_LAYER_ID)) {
        map.addLayer({
          id: BOUNDARY_FILL_LAYER_ID,
          type: "fill",
          source: BOUNDARY_SOURCE_ID,
          paint: { "fill-color": accent, "fill-opacity": 0.12 },
        });
      }
      if (!map.getLayer(BOUNDARY_GLOW_LAYER_ID)) {
        map.addLayer({
          id: BOUNDARY_GLOW_LAYER_ID,
          type: "line",
          source: BOUNDARY_SOURCE_ID,
          layout: { "line-join": "round", "line-cap": "round" },
          paint: { "line-color": accent, "line-width": 9, "line-blur": 4, "line-opacity": 0.45 },
        });
      }
      if (!map.getLayer(BOUNDARY_LINE_LAYER_ID)) {
        map.addLayer({
          id: BOUNDARY_LINE_LAYER_ID,
          type: "line",
          source: BOUNDARY_SOURCE_ID,
          layout: { "line-join": "round", "line-cap": "round" },
          paint: { "line-color": accent, "line-width": 2.5, "line-opacity": 0.95 },
        });
      }
    };
    if (map.isStyleLoaded()) addBoundaryLayers();
    else map.once("style.load", addBoundaryLayers);

    mapRef.current = map;
    setMapReady(true);

    return () => {
      markerRef.current?.remove();
      markerRef.current = null;
      map.remove();
      mapRef.current = null;
      setMapReady(false);
    };
    // Theme is handled by the effect below; the map is only constructed once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keep the basemap + boundary accent in sync with the resolved theme.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const mode = resolveBasemapMode(theme);
    const apply = () => {
      applyBasemapMode(map, mode);
      const accent = ACCENT[mode];
      if (map.getLayer(BOUNDARY_FILL_LAYER_ID)) map.setPaintProperty(BOUNDARY_FILL_LAYER_ID, "fill-color", accent);
      if (map.getLayer(BOUNDARY_GLOW_LAYER_ID)) map.setPaintProperty(BOUNDARY_GLOW_LAYER_ID, "line-color", accent);
      if (map.getLayer(BOUNDARY_LINE_LAYER_ID)) map.setPaintProperty(BOUNDARY_LINE_LAYER_ID, "line-color", accent);
    };
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [theme, mapReady]);

  // Fly to + marker for the selected postal point.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const apply = () => {
      markerRef.current?.remove();
      markerRef.current = null;
      if (!point) return;
      const el = buildHighlightMarkerElement(point.label);
      markerRef.current = new Marker({ element: el, anchor: "center" }).setLngLat([point.lon, point.lat]).addTo(map);
      map.flyTo({ center: [point.lon, point.lat], zoom: 14, essential: true, duration: 900 });
    };
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [point, mapReady]);

  // Draw (or clear) the admin boundary glow.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const apply = () => {
      const source = map.getSource(BOUNDARY_SOURCE_ID) as GeoJSONSource | undefined;
      if (!source) return;
      if (!geometry) {
        source.setData(EMPTY_FEATURE_COLLECTION);
        return;
      }
      source.setData({ type: "FeatureCollection", features: [geometry] });
      const bounds = featureBounds(geometry);
      if (bounds) map.fitBounds(bounds, { padding: 48, maxZoom: 15, duration: 900 });
    };
    if (map.isStyleLoaded()) apply();
    else map.once("style.load", apply);
  }, [geometry, mapReady]);

  return <div ref={containerRef} className="h-80 w-full overflow-hidden rounded-lg border border-border sm:h-96" />;
}
