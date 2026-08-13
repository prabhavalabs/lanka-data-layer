import { resolveBasemapMode } from "@/components/map/basemap";
import { POPULATION_LAYER_ID, POPULATION_RAMP_CSS } from "@/components/map/population-layer";
import { glassPanelStyle, MONO_FONT, SANS_FONT, TEXT3 } from "@/components/explore/glass";
import { useTheme } from "@/components/theme-provider";
import { useLayerStore } from "@/stores/layer-store";

const PANEL_WIDTH = 212;

/**
 * Bottom-right legend for the population-3d ramp. Renders nothing unless
 * that layer is visible — per the Explore design, the legend is
 * population-only; every other layer's color lives in the layer panel's
 * swatches instead of being duplicated here.
 */
export function MapLegend() {
  const visibility = useLayerStore((s) => s.visibility);
  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);

  if (!(visibility[POPULATION_LAYER_ID] ?? false)) return null;

  return (
    <div
      className="pointer-events-none absolute bottom-9 right-3 z-20"
      style={{ width: PANEL_WIDTH, fontFamily: SANS_FONT }}
    >
      <div className="pointer-events-auto rounded-xl border px-3.5 py-3" style={glassPanelStyle(mode)}>
        <div
          className="mb-2 text-[10px] font-semibold uppercase"
          style={{ color: TEXT3, letterSpacing: "0.08em", fontFamily: MONO_FONT }}
        >
          Population density
        </div>
        <div className="h-2 w-full rounded-full" style={{ background: POPULATION_RAMP_CSS }} />
        <div className="mt-1.5 flex justify-between text-[10px]" style={{ color: TEXT3, fontFamily: MONO_FONT }}>
          <span>sparse</span>
          <span>dense</span>
        </div>
        <div className="mt-2 text-[9px] leading-snug" style={{ color: TEXT3, fontFamily: MONO_FONT }}>
          WorldPop 1 km · UN-adjusted
          <br />
          Warmer areas are denser
        </div>
      </div>
    </div>
  );
}
