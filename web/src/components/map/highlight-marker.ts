import { ACCENT, BORDER, MONO_FONT, TEXT } from "@/components/explore/glass";

/**
 * Builds the DOM element for a point highlight's pulsing marker (see
 * map-store.ts's MapHighlight, consumed by MapView). A plain
 * maplibregl.Marker with a styled element rather than a deck.gl layer —
 * this is one marker at a time, not a data layer.
 *
 * The pulse ring uses Tailwind's built-in `ping` keyframes (global, from
 * Tailwind's preflight — not something index.css defines) but not the
 * `animate-ping` utility class itself, since that utility hardcodes a 1s
 * duration and the Explore design calls for a slower 1.8s pulse; setting
 * `animation` inline with the same keyframe name overrides just the
 * timing, referencing the same global `@keyframes ping` the utility would.
 */
export function buildHighlightMarkerElement(label?: string): HTMLDivElement {
  const wrapper = document.createElement("div");
  wrapper.className = "flex flex-col items-center gap-1";

  const dotWrap = document.createElement("span");
  dotWrap.className = "relative flex size-4 items-center justify-center";

  const ping = document.createElement("span");
  ping.className = "absolute inline-flex h-full w-full rounded-full opacity-75";
  ping.style.backgroundColor = ACCENT;
  ping.style.animation = "ping 1.8s cubic-bezier(0, 0, 0.2, 1) infinite";

  const dot = document.createElement("span");
  dot.className = "relative inline-flex size-2.5 rounded-full";
  dot.style.backgroundColor = ACCENT;
  dot.style.boxShadow = "0 0 0 2px var(--bg, #07090C)";

  dotWrap.append(ping, dot);
  wrapper.append(dotWrap);

  if (label) {
    const pill = document.createElement("span");
    pill.className = "whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium shadow";
    pill.style.background = "var(--bg2, rgba(13, 17, 23, 0.85))";
    pill.style.borderColor = BORDER;
    pill.style.color = TEXT;
    pill.style.fontFamily = MONO_FONT;
    pill.textContent = label;
    wrapper.append(pill);
  }

  return wrapper;
}
