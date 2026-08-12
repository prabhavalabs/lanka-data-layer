import { resolveBasemapMode } from "@/components/map/basemap";
import { glassPanelStyle, MONO_FONT, TEXT2 } from "@/components/explore/glass";
import { useTheme } from "@/components/theme-provider";

export interface ExploreReadoutProps {
  lat: number;
  lon: number;
  zoom: number;
}

/** Bottom-left mono coordinate readout — MapView keeps it updated on mousemove/zoom. */
export function ExploreReadout({ lat, lon, zoom }: ExploreReadoutProps) {
  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);

  return (
    <div className="pointer-events-none absolute bottom-9 left-3 z-20">
      <div
        className="pointer-events-auto rounded-xl border px-3 py-1.5 text-[11px] tabular-nums"
        style={{ ...glassPanelStyle(mode), fontFamily: MONO_FONT, color: TEXT2 }}
      >
        lat {lat.toFixed(4)} · lon {lon.toFixed(4)} · z {zoom.toFixed(1)}
      </div>
    </div>
  );
}
