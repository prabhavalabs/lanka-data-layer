import type { CSSProperties } from "react";

import type { BasemapMode } from "@/components/map/basemap";

/**
 * The Explore surface's "glass panel" recipe (approved design — see
 * DESIGN-NOTES.md's "Explore map mock" section) plus the semantic
 * text/accent tokens it's composed with.
 *
 * index.css (another agent's file, edited concurrently with this one) has
 * since landed the design tokens — but under --ink/--ink2/--ink3/--brand/
 * --brand2 rather than the --text/--text2/--text3/--accent/--accent2 names
 * this task brief was written against. Notably --accent/--accent-foreground
 * DO already exist there, but as shadcn's generic "muted highlight surface"
 * pair (aliased to --bg4, a neutral gray) — not the brand color — so a
 * `var(--accent, <fallback>)` reference would silently resolve to that gray
 * instead of falling through to the fallback. Every token below points at
 * the real names, still with the dark-first literal fallback in case a
 * name changes again.
 */

const GLASS_DARK = {
  background: "rgba(13, 17, 23, 0.72)",
  border: "rgba(120, 140, 160, 0.16)",
};

const GLASS_LIGHT = {
  background: "rgba(255, 255, 255, 0.72)",
  border: "rgba(23, 27, 32, 0.12)",
};

/** Inline style for a floating glass panel — background/border-color/blur/shadow. Pair with `rounded-xl border` (or your own radius/border-width classes) for the rest. */
export function glassPanelStyle(mode: BasemapMode): CSSProperties {
  const palette = mode === "dark" ? GLASS_DARK : GLASS_LIGHT;
  return {
    backgroundColor: palette.background,
    borderColor: palette.border,
    backdropFilter: "blur(14px)",
    WebkitBackdropFilter: "blur(14px)",
    boxShadow: "0 10px 36px rgba(0, 0, 0, 0.45)",
  };
}

/** A touch more opaque than a static panel — for dropdowns/menus that need to stay legible while map content scrolls behind them. */
export function glassSurfaceColor(mode: BasemapMode): string {
  return mode === "dark" ? "rgba(13, 17, 23, 0.92)" : "rgba(255, 255, 255, 0.92)";
}

export function glassBorderColor(mode: BasemapMode): string {
  return mode === "dark" ? GLASS_DARK.border : GLASS_LIGHT.border;
}

// Semantic tokens, each with the dark-first literal fallback from the
// design tokens table — see this file's doc comment above for why these
// point at --ink*/--brand* rather than --text*/--accent*.
export const TEXT = "var(--ink, #E8EDF2)";
export const TEXT2 = "var(--ink2, #9AA7B4)";
export const TEXT3 = "var(--ink3, #66737F)";
export const ACCENT = "var(--brand, #D8446B)";
export const ACCENT2 = "var(--brand2, #E5537A)";
export const BORDER = "var(--border, rgba(120, 140, 160, 0.16))";

export const MONO_FONT = "'Geist Mono', ui-monospace, SFMono-Regular, Menlo, monospace";
export const SANS_FONT = "'Geist', ui-sans-serif, system-ui, sans-serif";

/**
 * CSS custom-property overrides, meant to be spread into the `style` of a
 * wrapper around the (unmodified) Omnibox component. Its markup uses plain
 * shadcn-style utility classes — bg-background, bg-popover, border-input,
 * text-muted-foreground, etc. — which Tailwind's `@theme inline` block maps
 * straight through to `var(--background)`, `var(--popover)`, and so on.
 * CSS custom properties resolve `var()` lazily at the point of use, using
 * whatever value is nearest in the cascade at that element — so redefining
 * --background/--popover/--border/... on an ancestor is enough to retheme
 * every one of Omnibox's classes without touching its source (which is out
 * of scope here) or needing to know its internal class names. Also tightens
 * --radius so its rounded-md/rounded-lg corners match the glass panels'
 * 12px radius.
 */
export function omniboxVariableOverrides(mode: BasemapMode): CSSProperties {
  const glass = mode === "dark" ? GLASS_DARK : GLASS_LIGHT;
  const vars: Record<string, string> =
    mode === "dark"
      ? {
          "--radius": "0.75rem",
          "--background": "transparent",
          "--foreground": TEXT,
          "--popover": glassSurfaceColor("dark"),
          "--popover-foreground": TEXT,
          "--border": glass.border,
          "--input": glass.border,
          "--muted": "rgba(120, 140, 160, 0.12)",
          "--muted-foreground": TEXT2,
          "--accent": "rgba(216, 68, 107, 0.16)",
          "--accent-foreground": TEXT,
          "--ring": "rgba(216, 68, 107, 0.5)",
        }
      : {
          "--radius": "0.75rem",
          "--background": "transparent",
          "--foreground": TEXT,
          "--popover": glassSurfaceColor("light"),
          "--popover-foreground": TEXT,
          "--border": glass.border,
          "--input": glass.border,
          "--muted": "rgba(23, 27, 32, 0.06)",
          "--muted-foreground": TEXT2,
          "--accent": "rgba(141, 21, 58, 0.1)",
          "--accent-foreground": TEXT,
          "--ring": "rgba(141, 21, 58, 0.5)",
        };
  return vars as CSSProperties;
}
