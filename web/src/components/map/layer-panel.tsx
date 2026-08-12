import * as React from "react";

import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { LAYER_GROUP_LABELS, LAYER_REGISTRY, type LayerGroup } from "@/components/map/layer-registry";
import { POPULATION_LAYER_ID } from "@/components/map/population-layer";
import { useLayerStore } from "@/stores/layer-store";
import { cn } from "@/lib/utils";

const GROUP_ORDER: LayerGroup[] = ["boundaries", "electoral", "infrastructure", "environment", "population"];

/**
 * Floating left-rail panel listing every LAYER_REGISTRY entry, grouped and
 * toggle-able. Purely a view over layer-store — adding a layer to the
 * registry is enough for it to show up here, no changes needed in this file.
 */
export function LayerPanel() {
  const visibility = useLayerStore((s) => s.visibility);
  const toggleLayer = useLayerStore((s) => s.toggleLayer);
  const [collapsed, setCollapsed] = React.useState(false);

  return (
    <div className="pointer-events-none absolute left-3 top-3 z-20 max-h-[calc(100%-1.5rem)] w-64">
      <Card className="pointer-events-auto flex max-h-full flex-col gap-0 overflow-hidden py-0 shadow-md">
        <button
          type="button"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
          className="flex items-center justify-between px-3 py-2.5 text-left"
        >
          <span className="text-sm font-semibold">Layers</span>
          <span className="text-xs text-muted-foreground">{collapsed ? "Show" : "Hide"}</span>
        </button>
        {!collapsed && (
          <div className="overflow-y-auto border-t border-border px-2 py-2">
            {GROUP_ORDER.map((group) => {
              const entries = LAYER_REGISTRY.filter((l) => l.group === group);
              if (entries.length === 0) return null;
              return (
                <div key={group} className="py-1.5">
                  <div className="mb-1 px-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                    {LAYER_GROUP_LABELS[group]}
                  </div>
                  <div className="flex flex-col gap-0.5">
                    {entries.map((entry) => {
                      const checked = visibility[entry.id] ?? false;
                      return (
                        <button
                          key={entry.id}
                          type="button"
                          role="switch"
                          aria-checked={checked}
                          onClick={() => toggleLayer(entry.id)}
                          className={cn(
                            "flex w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-muted",
                            entry.id === POPULATION_LAYER_ID && "font-medium"
                          )}
                        >
                          <span>{entry.title}</span>
                          <Switch checked={checked} />
                        </button>
                      );
                    })}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </Card>
    </div>
  );
}
