import * as React from "react";

import { resolveBasemapMode } from "@/components/map/basemap";
import { LAYER_REGISTRY } from "@/components/map/layer-registry";
import { POPULATION_LAYER_ID } from "@/components/map/population-layer";
import { ACCENT, glassPanelStyle, MONO_FONT, SANS_FONT, TEXT, TEXT3 } from "@/components/explore/glass";
import { useTheme } from "@/components/theme-provider";
import { useLayerStore } from "@/stores/layer-store";

const PANEL_WIDTH = 228;
const TOGGLE_TRACK_OFF = "rgba(120, 140, 160, 0.28)";

interface LayerRowProps {
  title: string;
  swatch: string;
  checked: boolean;
  emphasize?: boolean;
  onToggle: () => void;
}

/**
 * One layer row: color swatch + name + a bespoke 26x15 iOS-style toggle.
 * Not the shared components/ui/switch.tsx — that component's track size is
 * fixed (20x36) and its thumb isn't parameterized, so reusing it here would
 * either fight its classes or leave the thumb mis-sized against this
 * panel's smaller 26x15 track. This is a self-contained presentational
 * replacement, same "whole row is the interactive control" pattern
 * (role="switch"/aria-checked on the button, the track/thumb are purely
 * visual) as the shared Switch's own doc comment describes.
 */
function LayerRow({ title, swatch, checked, emphasize, onToggle }: LayerRowProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={onToggle}
      className="flex w-full items-center gap-2.5 rounded-md px-1.5 py-[7px] text-left transition-colors hover:bg-[rgba(120,140,160,0.10)]"
    >
      <span aria-hidden="true" className="size-3.5 shrink-0 rounded-[3px]" style={{ background: swatch }} />
      <span className="flex-1 truncate text-[12.5px]" style={{ color: TEXT, fontWeight: emphasize ? 600 : 400 }}>
        {title}
      </span>
      <span
        aria-hidden="true"
        className="relative inline-flex h-[15px] w-[26px] shrink-0 items-center rounded-full transition-colors duration-150"
        style={{ background: checked ? ACCENT : TOGGLE_TRACK_OFF }}
      >
        <span
          className="inline-block size-[11px] rounded-full bg-white shadow transition-transform duration-150"
          style={{ transform: checked ? "translateX(13px)" : "translateX(2px)" }}
        />
      </span>
    </button>
  );
}

/**
 * Floating glass panel, top-left below the header pill, listing every
 * LAYER_REGISTRY entry as a swatch + name + toggle row. Purely a view over
 * layer-store — adding a layer to the registry is enough for it to show up
 * here, no changes needed in this file.
 */
export function LayerPanel() {
  const visibility = useLayerStore((s) => s.visibility);
  const toggleLayer = useLayerStore((s) => s.toggleLayer);
  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);
  const [collapsed, setCollapsed] = React.useState(false);

  return (
    <div
      className="pointer-events-none absolute left-3 top-[62px] z-20"
      style={{ width: PANEL_WIDTH, fontFamily: SANS_FONT }}
    >
      <div className="pointer-events-auto overflow-hidden rounded-xl border" style={glassPanelStyle(mode)}>
        <button
          type="button"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
          className="flex w-full items-center justify-between px-3 py-2"
        >
          <span
            className="text-[10px] font-semibold uppercase"
            style={{ color: TEXT3, letterSpacing: "0.08em", fontFamily: MONO_FONT }}
          >
            Layers
          </span>
          <span className="text-[10px]" style={{ color: TEXT3, fontFamily: MONO_FONT }}>
            {collapsed ? "Show" : "Hide"}
          </span>
        </button>
        {!collapsed && (
          <div className="flex flex-col gap-0.5 px-1.5 pb-2">
            {LAYER_REGISTRY.map((entry) => (
              <LayerRow
                key={entry.id}
                title={entry.title}
                swatch={entry.swatchColor}
                checked={visibility[entry.id] ?? false}
                emphasize={entry.id === POPULATION_LAYER_ID}
                onToggle={() => toggleLayer(entry.id)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
