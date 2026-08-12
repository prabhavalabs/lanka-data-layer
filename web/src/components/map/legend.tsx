import { Card } from "@/components/ui/card";
import { useTheme } from "@/components/theme-provider";
import { resolveBasemapMode } from "@/components/map/basemap";
import { LAYER_REGISTRY } from "@/components/map/layer-registry";
import { populationRampCss, POPULATION_LAYER_ID } from "@/components/map/population-layer";
import { useLayerStore } from "@/stores/layer-store";

/**
 * Bottom-right legend: one swatch row per currently-visible registry entry,
 * with population-3d rendered as a gradient ramp bar instead of a dot.
 * Renders nothing when no layer is visible.
 */
export function MapLegend() {
  const visibility = useLayerStore((s) => s.visibility);
  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);

  const visibleEntries = LAYER_REGISTRY.filter((entry) => visibility[entry.id]);
  if (visibleEntries.length === 0) return null;

  return (
    <div className="pointer-events-none absolute bottom-9 right-3 z-20 max-w-56">
      <Card className="pointer-events-auto px-3 py-2.5 shadow-md">
        <div className="flex flex-col gap-2">
          {visibleEntries.map((entry) =>
            entry.id === POPULATION_LAYER_ID ? (
              <div key={entry.id} className="flex flex-col gap-1">
                <span className="text-xs font-medium">{entry.title}</span>
                <div className="h-2 w-full rounded-full" style={{ background: populationRampCss(mode) }} />
                <div className="flex justify-between text-[10px] text-muted-foreground">
                  <span>low</span>
                  <span>high</span>
                </div>
              </div>
            ) : (
              <div key={entry.id} className="flex items-center gap-2 text-xs">
                <span
                  className="size-2.5 shrink-0 rounded-full"
                  style={{ background: entry.kind === "maplibre" ? (entry.legendColor?.(mode) ?? "var(--muted-foreground)") : "var(--muted-foreground)" }}
                />
                <span>{entry.title}</span>
              </div>
            )
          )}
        </div>
      </Card>
    </div>
  );
}
