import type { LayerSpecification, SourceSpecification } from "maplibre-gl";

/**
 * One togglable map layer: a source plus the MapLibre layers drawn from it.
 * New layers are added to the map — and to any future layer-toggle UI —
 * purely by appending an entry to LAYER_REGISTRY below, never by writing
 * imperative addSource/addLayer calls in a component. MapView just walks
 * this array.
 */
export interface LayerDefinition {
  /** Also used as the MapLibre source id — must be unique in the registry. */
  id: string;
  title: string;
  source: SourceSpecification;
  layers: LayerSpecification[];
  defaultVisible: boolean;
}

/**
 * Empty for now — no PMTiles artifacts exist yet. Per docs/architecture.md
 * §3, the foundry emits `tiles/<layer>.pmtiles` for: admin, electoral,
 * roads, railways, water, pois. Each becomes one LayerDefinition here once
 * its artifact is published; the zoom-graduated paint expressions tuned in
 * the ceylon-hub reference (circle-radius / text-size / line-width
 * interpolated on ["zoom"], per road/place class) are the values to lift
 * into those entries' `layers` specs.
 */
export const LAYER_REGISTRY: LayerDefinition[] = [];
