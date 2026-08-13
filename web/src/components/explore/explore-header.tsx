import { Link } from "react-router-dom";

import { resolveBasemapMode } from "@/components/map/basemap";
import { CompassIcon } from "@/components/explore/compass-icon";
import { ACCENT, BORDER, glassPanelStyle, SANS_FONT, TEXT, TEXT2, TEXT3 } from "@/components/explore/glass";
import { useTheme } from "@/components/theme-provider";

/**
 * Top-left brand pill: accent logo mark, "Lanka Data Layer / Explore", and a
 * link back to the docs surface at `/` (see App.tsx, out of scope here —
 * docs now lives at the app's index route).
 */
export function ExploreHeader() {
  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);

  return (
    <div className="pointer-events-none absolute left-3 top-3 z-30" style={{ fontFamily: SANS_FONT }}>
      <div
        className="pointer-events-auto flex items-center gap-2.5 rounded-xl border py-1.5 pl-1.5 pr-3.5"
        style={glassPanelStyle(mode)}
      >
        <span
          className="flex size-7 shrink-0 items-center justify-center rounded-lg"
          style={{ background: ACCENT }}
        >
          <CompassIcon className="size-4 text-white" />
        </span>
        <span className="flex items-baseline gap-1.5 whitespace-nowrap text-[13px]">
          <span className="font-semibold" style={{ color: TEXT }}>
            Lanka Data Layer
          </span>
          <span style={{ color: TEXT3 }}>/</span>
          <span style={{ color: TEXT2 }}>Explore</span>
        </span>
        <span className="mx-0.5 h-4 w-px shrink-0" style={{ background: BORDER }} />
        <Link
          to="/"
          className="whitespace-nowrap text-[12px] font-medium transition-opacity hover:opacity-80"
          style={{ color: TEXT2 }}
        >
          ← Docs
        </Link>
      </div>
    </div>
  );
}
