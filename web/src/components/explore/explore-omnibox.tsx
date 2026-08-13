import * as React from "react";

import { resolveBasemapMode } from "@/components/map/basemap";
import { glassPanelStyle, omniboxVariableOverrides, SANS_FONT } from "@/components/explore/glass";
import { Omnibox } from "@/components/search/omnibox";
import { useTheme } from "@/components/theme-provider";

const OMNIBOX_PLACEHOLDER = "Search the island — city, district, or lat, lon";

/**
 * Glass-styled wrapper around the shared Omnibox (owned by
 * components/search/**, out of scope to edit here — reused as-is per the
 * task brief). Omnibox takes no styling props and its placeholder text is
 * hardcoded, so:
 *
 * - Colors/surfaces are retheme'd by overriding the CSS custom properties
 *   its own utility classes already resolve through (see
 *   omniboxVariableOverrides' doc comment in glass.ts) — no knowledge of
 *   its internal class names required.
 * - The placeholder is set imperatively on the real <input> after mount.
 *   This only changes what's displayed (the DOM attribute), not the
 *   component's behavior — a deliberate, narrow exception to "don't touch
 *   the DOM imperatively," made because there's no other hook available
 *   and the alternative (forking the component) would violate the
 *   "reuse as-is" boundary far more.
 */
export function ExploreOmnibox() {
  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);
  const wrapperRef = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    const input = wrapperRef.current?.querySelector<HTMLInputElement>('input[role="combobox"]');
    if (input) input.placeholder = OMNIBOX_PLACEHOLDER;
  }, []);

  return (
    <div className="pointer-events-none absolute left-1/2 top-3 z-30 w-[380px] -translate-x-1/2">
      <div
        ref={wrapperRef}
        className="pointer-events-auto rounded-xl border px-1"
        style={{ ...glassPanelStyle(mode), ...omniboxVariableOverrides(mode), fontFamily: SANS_FONT }}
      >
        <Omnibox />
      </div>
    </div>
  );
}
