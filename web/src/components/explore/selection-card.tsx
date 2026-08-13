import * as React from "react";

import { resolveBasemapMode } from "@/components/map/basemap";
import { ACCENT, BORDER, glassPanelStyle, MONO_FONT, SANS_FONT, TEXT, TEXT2, TEXT3 } from "@/components/explore/glass";
import { ADMIN_LEVEL_LABELS } from "@/components/explore/selection-format";
import { SecondarySections, SelectionSections } from "@/components/explore/selection-sections";
import { ShimmerBlock } from "@/components/explore/selection-widgets";
import { useTheme } from "@/components/theme-provider";
import { useIsMobileViewport, usePrefersReducedMotion } from "@/hooks/use-media-query";
import { dedupeSources, useSelectionCore, useSelectionSecondary, type SelectionData } from "@/hooks/use-selection-data";
import { LOOKUP_TYPE_LABELS } from "@/lib/lookup";
import { useMapStore, type Selection } from "@/stores/map-store";

// Below the top-right zoom control (matches map-view.tsx's FIT_PADDING.top,
// which reserves the same clearance for the camera fit) and clear of the
// left edge's header/layer panel furniture.
const DESKTOP_TOP = 76;
const DESKTOP_WIDTH = 340;
// The legend (map/legend.tsx) sits `bottom-9 right-3` and, with its title +
// gradient bar + labels + two-line attribution, runs roughly 100px tall —
// this reserves 36px (its own offset) + that + a buffer so the card's
// bottom edge always caps above the legend's top, per the task brief,
// without the two ever needing to coordinate directly.
const DESKTOP_BOTTOM_CLEARANCE = 172;
const DESKTOP_MAX_HEIGHT = `calc(100dvh - ${DESKTOP_TOP}px - ${DESKTOP_BOTTOM_CLEARANCE}px)`;
const MOBILE_MAX_HEIGHT = "56dvh";

const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

function CloseIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden="true" className="size-3.5">
      <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

function ChevronIcon({ up }: { up: boolean }) {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden="true" className="size-3.5" style={{ transform: up ? "rotate(180deg)" : undefined }}>
      <path d="M4 6l4 4 4-4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function PinIcon({ filled }: { filled: boolean }) {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden="true" className="size-3.5">
      <path
        d="M8 1.5c-2.02 0-3.6 1.6-3.6 3.6 0 2.55 3.6 8.4 3.6 8.4s3.6-5.85 3.6-8.4c0-2-1.58-3.6-3.6-3.6Z"
        stroke="currentColor"
        strokeWidth="1.4"
        fill={filled ? "currentColor" : "none"}
      />
      <circle cx="8" cy="5.1" r="1.3" fill={filled ? "var(--bg2, #0E1217)" : "currentColor"} stroke={filled ? "none" : "currentColor"} strokeWidth="1.1" />
    </svg>
  );
}

function IconButton({
  onClick,
  active,
  title,
  children,
}: {
  onClick: () => void;
  active?: boolean;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      className="flex size-6 shrink-0 items-center justify-center rounded-md transition-colors hover:bg-[rgba(120,140,160,0.16)]"
      style={{ color: active ? ACCENT : TEXT2 }}
    >
      {children}
    </button>
  );
}

function TypeChip({ label }: { label: string }) {
  return (
    <span
      className="shrink-0 rounded-full border px-1.5 py-0.5 text-[9.5px] font-semibold uppercase"
      style={{ borderColor: BORDER, color: TEXT3, letterSpacing: "0.06em" }}
    >
      {label}
    </span>
  );
}

function SkeletonBody() {
  return (
    <div className="flex flex-col gap-4 px-4 py-3.5">
      <div className="space-y-1.5">
        <ShimmerBlock className="h-2.5 w-16" />
        <ShimmerBlock className="h-6 w-28" />
      </div>
      <div className="space-y-1.5">
        <ShimmerBlock className="h-2.5 w-20" />
        <ShimmerBlock className="h-3.5 w-full" />
      </div>
      <div className="space-y-1.5">
        <ShimmerBlock className="h-2.5 w-24" />
        <ShimmerBlock className="h-2 w-full rounded-full" />
        <ShimmerBlock className="h-2 w-2/3 rounded-full" />
      </div>
    </div>
  );
}

function ErrorBody({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className="flex flex-col items-start gap-2 px-4 py-4">
      <div className="text-[12.5px]" style={{ color: TEXT2 }}>
        {message}
      </div>
      <button
        type="button"
        onClick={onRetry}
        className="rounded-md border px-2.5 py-1 text-[11.5px] font-medium transition-colors hover:bg-[rgba(120,140,160,0.12)]"
        style={{ borderColor: BORDER, color: TEXT }}
      >
        Retry
      </button>
    </div>
  );
}

interface HeaderInfo {
  title: string;
  secondary: string[];
  typeLabel: string;
  code: string | undefined;
}

function headerInfo(selection: NonNullable<Selection>, data: SelectionData | undefined): HeaderInfo {
  if (data?.type === "admin") {
    const secondary = [data.unit.name_si, data.unit.name_ta].filter((s): s is string => !!s);
    return { title: data.unit.name, secondary, typeLabel: ADMIN_LEVEL_LABELS[data.level], code: data.unit.pcode };
  }
  if (data?.type === "postal") {
    return { title: data.name, secondary: [], typeLabel: LOOKUP_TYPE_LABELS.postal, code: data.code };
  }
  if (data?.type === "place") {
    return { title: data.label, secondary: data.sublabel ? [data.sublabel] : [], typeLabel: LOOKUP_TYPE_LABELS.place, code: data.pcode };
  }
  if (data?.type === "coordinate") {
    const name = data.reverse.gnd?.name ?? data.reverse.nearest_place?.name ?? null;
    return {
      title: name ?? `${data.lat.toFixed(4)}, ${data.lon.toFixed(4)}`,
      secondary: [],
      typeLabel: LOOKUP_TYPE_LABELS.coordinate,
      code: undefined,
    };
  }
  // Still loading / errored — fall back to whatever the selection itself carries.
  const typeLabel = LOOKUP_TYPE_LABELS[selection.type];
  if (selection.type === "coordinate") {
    return { title: selection.label ?? `${selection.lat.toFixed(4)}, ${selection.lon.toFixed(4)}`, secondary: [], typeLabel, code: undefined };
  }
  if (selection.type === "place") {
    return { title: selection.label, secondary: selection.sublabel ? [selection.sublabel] : [], typeLabel, code: selection.pcode };
  }
  return { title: selection.label ?? "…", secondary: [], typeLabel, code: selection.type === "admin" ? selection.pcode : selection.code };
}

/**
 * The selection detail card — desktop: floating glass panel, right side
 * below the zoom control, capped above the legend; mobile (<768px): a
 * two-state bottom sheet (collapsed header strip / expanded scrollable
 * body). Renders nothing when `selection` is null.
 *
 * Fetches its own data progressively (independent of the map highlight,
 * which MapView/Omnibox/map-selection.ts drive separately) — the "core"
 * query (useSelectionCore) renders the frame, header, and primary sections
 * the moment it lands; useSelectionSecondary then fills in the rest,
 * section by section, once core succeeds. Loading/error/success are all
 * handled here so callers just need to set `selection`.
 */
export function SelectionCard() {
  const selection = useMapStore((s) => s.selection);
  const collapsed = useMapStore((s) => s.collapsed);
  const pinned = useMapStore((s) => s.pinned);
  const clearSelection = useMapStore((s) => s.clearSelection);
  const toggleCollapsed = useMapStore((s) => s.toggleCollapsed);
  const togglePinned = useMapStore((s) => s.togglePinned);

  const { theme } = useTheme();
  const mode = resolveBasemapMode(theme);
  const isMobile = useIsMobileViewport();
  const reducedMotion = usePrefersReducedMotion();

  const state = useSelectionCore(selection);
  const secondary = useSelectionSecondary(selection, state);
  // The footer's source list is the union of the core response's
  // attribution plus every secondary section's own — recomputed (and thus
  // visibly growing) as each secondary section lands, per the task brief.
  const allSources = React.useMemo(
    () => (state.status === "success" ? dedupeSources([...state.data.source, ...secondary.flatMap((s) => s.source)]) : []),
    [state, secondary]
  );

  if (!selection) return null;

  const data = state.status === "success" ? state.data : undefined;
  const header = headerInfo(selection, data);

  const bodyMaxHeight = isMobile ? MOBILE_MAX_HEIGHT : DESKTOP_MAX_HEIGHT;
  const wrapperClass = isMobile
    ? "pointer-events-none fixed inset-x-0 bottom-0 z-30 flex justify-center"
    : "pointer-events-none absolute right-3 z-30";
  const wrapperStyle: React.CSSProperties = isMobile ? {} : { top: DESKTOP_TOP, width: DESKTOP_WIDTH };

  return (
    <div className={wrapperClass} style={{ ...wrapperStyle, fontFamily: SANS_FONT }}>
      <div
        className={
          "pointer-events-auto w-full overflow-hidden border " +
          (isMobile ? "rounded-t-2xl" : "rounded-xl") +
          (reducedMotion ? "" : " animate-card-in")
        }
        style={{ ...glassPanelStyle(mode), maxWidth: isMobile ? 480 : DESKTOP_WIDTH }}
      >
        {/* Mobile drag-handle affordance — tapping (or the header strip below) toggles collapsed/expanded. */}
        {isMobile && (
          <button type="button" onClick={toggleCollapsed} aria-label={collapsed ? "Expand" : "Collapse"} className="flex w-full justify-center py-1.5">
            <span className="h-1 w-9 rounded-full" style={{ background: BORDER }} />
          </button>
        )}

        {/* Header strip — always visible, even when collapsed. */}
        <div className="flex items-center gap-2 px-3.5 py-2.5">
          <TypeChip label={header.typeLabel} />
          <span className="min-w-0 flex-1 truncate text-[14.5px] font-semibold" style={{ color: TEXT }}>
            {header.title}
          </span>
          <div className="flex shrink-0 items-center gap-0.5">
            <IconButton onClick={togglePinned} active={pinned} title={pinned ? "Unpin" : "Pin card open"}>
              <PinIcon filled={pinned} />
            </IconButton>
            <IconButton onClick={toggleCollapsed} title={collapsed ? "Expand" : "Collapse"}>
              <ChevronIcon up={!collapsed} />
            </IconButton>
            <IconButton onClick={clearSelection} title="Close">
              <CloseIcon />
            </IconButton>
          </div>
        </div>

        {/* Collapsible region — secondary header line + scrollable body, animates max-height. */}
        <div
          style={{
            maxHeight: collapsed ? 0 : bodyMaxHeight,
            overflowY: collapsed ? "hidden" : "auto",
            transition: reducedMotion ? undefined : `max-height 320ms ${EASE}`,
          }}
        >
          {(header.secondary.length > 0 || header.code) && (
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 px-3.5 pb-2.5">
              {header.secondary.map((s) => (
                <span key={s} className="text-[12px]" style={{ color: TEXT2 }}>
                  {s}
                </span>
              ))}
              {header.code && (
                <span className="rounded border px-1 py-px text-[10.5px] tabular-nums" style={{ borderColor: BORDER, color: TEXT3, fontFamily: MONO_FONT }}>
                  {header.code}
                </span>
              )}
            </div>
          )}

          {state.status === "loading" && <SkeletonBody />}
          {state.status === "error" && <ErrorBody message={state.error.message} onRetry={state.retry} />}
          {state.status === "success" && (
            <div className="flex flex-col gap-3.5 border-t px-3.5 py-3.5" style={{ borderColor: BORDER }}>
              <SelectionSections data={state.data} reducedMotion={reducedMotion} />
              <SecondarySections sections={secondary} reducedMotion={reducedMotion} />
              {allSources.length > 0 && (
                <div className="flex flex-wrap gap-1.5 border-t pt-3" style={{ borderColor: BORDER }}>
                  {allSources.map((s) => (
                    <span
                      key={s.name + s.license}
                      title={`${s.name} — ${s.license}`}
                      className="max-w-full truncate rounded-full border px-2 py-0.5 text-[10px]"
                      style={{ borderColor: BORDER, color: TEXT3 }}
                    >
                      {s.name} · {s.license}
                    </span>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
