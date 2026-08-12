import * as React from "react";
import { AttributionControl, Map as MaplibreMap, NavigationControl, ScaleControl } from "maplibre-gl";
import type { LngLatBoundsLike } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { GRID } from "@geopub/shared";

import { useTheme } from "@/components/theme-provider";
import { applyBasemapMode, basemapStyle, resolveBasemapMode } from "@/components/map/basemap";
import { LAYER_REGISTRY } from "@/components/map/layer-registry";
import { useLayerStore } from "@/stores/layer-store";
import { useMapStore } from "@/stores/map-store";

// Keep the camera within the canonical grid's coverage, padded a bit so
// panning near the coast doesn't feel clipped.
const CAMERA_PADDING_DEG = 1.5;
const CAMERA_BOUNDS: LngLatBoundsLike = [
  [GRID.lonMin - CAMERA_PADDING_DEG, GRID.latMin - CAMERA_PADDING_DEG],
  [GRID.lonMax + CAMERA_PADDING_DEG, GRID.latMax + CAMERA_PADDING_DEG],
];

/**
 * The MapLibre canvas. Deliberately small: layers come entirely from
 * LAYER_REGISTRY (currently empty — see that file), so there's no
 * per-dataset imperative wiring to grow this component over time.
 *
 * The map is the source of truth for camera position once mounted; it only
 * reads the store once, at construction, for the initial view (already
 * restored from the URL by useMapUrlSync before this mounts). After that it
 * pushes moveend updates back into the store one-way, for the URL sync to
 * pick up — nothing currently drives the camera programmatically, so there
 * is no store -> map effect to fight with.
 */
export function MapView() {
  const { theme } = useTheme();
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const mapRef = React.useRef<MaplibreMap | null>(null);
  const visibility = useLayerStore((s) => s.visibility);

  React.useEffect(() => {
    if (!containerRef.current || mapRef.current) return;

    const { center, zoom } = useMapStore.getState();
    const mode = resolveBasemapMode(theme);

    const map = new MaplibreMap({
      container: containerRef.current,
      style: basemapStyle(mode),
      center,
      zoom,
      minZoom: 5,
      maxZoom: 18,
      maxBounds: CAMERA_BOUNDS,
      attributionControl: false,
    });

    map.addControl(new AttributionControl({ compact: true }));
    map.addControl(new NavigationControl({ showCompass: true }), "top-right");
    map.addControl(new ScaleControl({ maxWidth: 120, unit: "metric" }), "bottom-left");

    const onMoveEnd = () => {
      const c = map.getCenter();
      useMapStore.getState().setView({ center: [c.lng, c.lat], zoom: map.getZoom() });
    };
    map.on("moveend", onMoveEnd);

    const addRegistryLayers = () => {
      for (const layer of LAYER_REGISTRY) {
        if (!map.getSource(layer.id)) {
          map.addSource(layer.id, layer.source);
        }
        for (const spec of layer.layers) {
          if (!map.getLayer(spec.id)) {
            map.addLayer(spec);
          }
        }
      }
    };
    if (map.isStyleLoaded()) addRegistryLayers();
    else map.once("load", addRegistryLayers);

    mapRef.current = map;

    return () => {
      map.off("moveend", onMoveEnd);
      map.remove();
      mapRef.current = null;
    };
    // Map is created once on mount; theme is handled by the effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Swap the basemap tiles when the resolved theme changes.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const apply = () => applyBasemapMode(map, resolveBasemapMode(theme));
    if (map.isStyleLoaded()) apply();
    else map.once("load", apply);
  }, [theme]);

  // Show/hide registry layers as the layer store changes.
  React.useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const apply = () => {
      for (const layer of LAYER_REGISTRY) {
        const value = visibility[layer.id] ? "visible" : "none";
        for (const spec of layer.layers) {
          if (map.getLayer(spec.id)) {
            map.setLayoutProperty(spec.id, "visibility", value);
          }
        }
      }
    };
    if (map.isStyleLoaded()) apply();
    else map.once("load", apply);
  }, [visibility]);

  return <div ref={containerRef} className="size-full" />;
}
