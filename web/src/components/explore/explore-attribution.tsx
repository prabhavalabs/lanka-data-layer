import { MONO_FONT, TEXT3 } from "@/components/explore/glass";

const ATTRIBUTION_TEXT =
  "© geoBoundaries CC BY · © OpenStreetMap contributors ODbL · GeoNames · WorldPop CC BY";

/**
 * Centered bottom attribution for the data layers themselves (admin
 * boundaries, places, population). The basemap's own required CARTO/OSM
 * tile attribution stays on MapLibre's native (compact) AttributionControl
 * — see map-view.tsx — rather than being folded in here, so removing this
 * text can never drop a legally-required notice.
 */
export function ExploreAttribution() {
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-1.5 z-10 flex justify-center px-3">
      <span
        className="text-center text-[9px] leading-none"
        style={{ fontFamily: MONO_FONT, color: TEXT3, textShadow: "0 1px 3px rgba(0, 0, 0, 0.4)" }}
      >
        {ATTRIBUTION_TEXT}
      </span>
    </div>
  );
}
