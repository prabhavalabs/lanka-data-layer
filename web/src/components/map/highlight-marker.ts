/**
 * Builds the DOM element for a point highlight's pulsing marker (see
 * map-store.ts's MapHighlight, consumed by MapView). A plain
 * maplibregl.Marker with a styled element rather than a deck.gl layer —
 * this is one marker at a time, not a data layer, and Tailwind's built-in
 * `animate-ping` utility gives the pulse for free.
 */
export function buildHighlightMarkerElement(label?: string): HTMLDivElement {
  const wrapper = document.createElement("div");
  wrapper.className = "flex flex-col items-center gap-1";

  const dotWrap = document.createElement("span");
  dotWrap.className = "relative flex size-4 items-center justify-center";

  const ping = document.createElement("span");
  ping.className = "absolute inline-flex h-full w-full animate-ping rounded-full bg-primary opacity-75";

  const dot = document.createElement("span");
  dot.className = "relative inline-flex size-2.5 rounded-full bg-primary ring-2 ring-background";

  dotWrap.append(ping, dot);
  wrapper.append(dotWrap);

  if (label) {
    const pill = document.createElement("span");
    pill.className =
      "whitespace-nowrap rounded-full border border-border bg-popover px-2 py-0.5 text-xs font-medium text-popover-foreground shadow";
    pill.textContent = label;
    wrapper.append(pill);
  }

  return wrapper;
}
