import * as React from "react";
import { NavLink } from "react-router-dom";

import { useTheme } from "@/components/theme-provider";
import { Omnibox } from "@/components/search/omnibox";
import { apiGetPayload } from "@/lib/api";
import { docsEndpointsByGroup, endpointNavLabel } from "@/lib/docs-spec";
import { cn } from "@/lib/utils";

/** Compass glyph, copied verbatim from the approved design's logo mark (geopub-docs.dc.html). */
function CompassMark() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <circle cx="7" cy="7" r="5.4" stroke="white" strokeWidth="1.4" />
      <path d="M7 1.6V4M7 10v2.4M1.6 7H4M10 7h2.4" stroke="white" strokeWidth="1.4" strokeLinecap="round" />
      <circle cx="7" cy="7" r="1.5" fill="white" />
    </svg>
  );
}

function MapMark() {
  return (
    <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M1.5 3.5L5 2l4 1.5L12.5 2v8.5L9 12 5 10.5 1.5 12V3.5z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
      <path d="M5 2v8.5M9 3.5V12" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}

const navItemClass = "flex items-center gap-1.5 rounded-md px-2.5 py-[5.5px] text-[13px] w-full text-left transition-colors hover:bg-bg4";

function GetChip() {
  return (
    <span className="shrink-0 rounded-[4px] bg-positive-soft px-[4.5px] py-[1.5px] font-mono text-[9px] font-semibold text-positive">
      GET
    </span>
  );
}

/** Health chip states for the footer status row, driven by a real GET /v1/health ping (docs/architecture.md §4's liveness probe). */
type HealthState = "checking" | "ok" | "fail";

function useApiHealth(): HealthState {
  const [state, setState] = React.useState<HealthState>("checking");

  React.useEffect(() => {
    const controller = new AbortController();
    apiGetPayload<{ status: string }>("/health", { signal: controller.signal })
      .then((payload) => setState(payload.status === "ok" ? "ok" : "fail"))
      .catch((err: unknown) => {
        if (err instanceof DOMException && err.name === "AbortError") return;
        setState("fail");
      });
    return () => controller.abort();
  }, []);

  return state;
}

function StatusRow() {
  const health = useApiHealth();
  const { resolvedTheme, setTheme } = useTheme();

  const dotClass = health === "ok" ? "bg-ok" : health === "fail" ? "bg-warn" : "bg-ink3";
  const label = health === "ok" ? "api · normal" : health === "fail" ? "api · unreachable" : "api · checking…";

  return (
    <div className="flex items-center gap-2 border-t border-border px-4 py-3">
      <span className={cn("size-[7px] shrink-0 rounded-full", dotClass)} aria-hidden="true" />
      <span className="text-[11.5px] text-ink3">{label}</span>
      <button
        type="button"
        title="Toggle theme"
        onClick={() => setTheme(resolvedTheme === "dark" ? "light" : "dark")}
        className="ml-auto rounded-md border border-border bg-bg3 px-2 py-[3px] font-mono text-[10.5px] text-ink2 transition-colors hover:border-line2 hover:text-ink"
      >
        {resolvedTheme === "dark" ? "◐ light" : "◑ dark"}
      </button>
    </div>
  );
}

export interface SidebarProps {
  /** Whether the mobile off-canvas sheet is open. Ignored at the `md` breakpoint and above, where the sidebar is always static. */
  mobileOpen: boolean;
  /** Called on backdrop click and after any nav item is followed, so the sheet closes once a destination is picked. */
  onClose: () => void;
}

/**
 * The app's left-hand navigation — the design's "docs sidebar": logo mark,
 * search affordance (the real Omnibox, reused as-is per its owner's
 * boundary — this just rehomes where it renders, not how it works), grouped
 * endpoint nav with GET chips, an Explore Map exit, and a footer status row.
 * This is now the one persistent shell for every route (docs home,
 * endpoint pages, /datasets, /elections, /map), replacing the old
 * Header+Sidebar pair. Below `md`, it's an off-canvas sheet (per the design's
 * responsive rule) instead of a static column — see app-shell.tsx for the
 * hamburger trigger that owns `mobileOpen`.
 */
export function Sidebar({ mobileOpen, onClose }: SidebarProps) {
  const groups = docsEndpointsByGroup();

  return (
    <>
      {mobileOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/50 md:hidden"
          onClick={onClose}
          aria-hidden="true"
        />
      )}
      <aside
        className={cn(
          "fixed inset-y-0 left-0 z-50 flex w-[250px] shrink-0 flex-col border-r border-border bg-bg2 transition-transform duration-200 ease-out md:static md:z-auto md:translate-x-0",
          mobileOpen ? "translate-x-0" : "-translate-x-full"
        )}
      >
      <div className="flex items-center gap-2.5 px-4 pb-3 pt-[18px]">
        <div className="grid size-[26px] shrink-0 place-items-center rounded-[7px] bg-brand text-white">
          <CompassMark />
        </div>
        <div className="text-[15.5px] font-semibold tracking-tight">Geopub</div>
        <div className="ml-auto rounded-full border border-line2 px-[7px] py-[1px] font-mono text-[10px] text-ink3">v1</div>
      </div>

      <div className="px-3 pb-1.5 [&_input]:h-8 [&_input]:text-[13px]">
        <Omnibox />
      </div>

      <nav className="flex flex-1 flex-col gap-4 overflow-y-auto px-3 pb-3 pt-1.5">
        <div className="flex flex-col gap-px">
          <div className="px-2.5 pb-[5px] text-[10.5px] font-semibold uppercase tracking-[0.09em] text-ink3">Getting started</div>
          <NavLink
            to="/"
            end
            onClick={onClose}
            className={({ isActive }) => cn(navItemClass, isActive ? "bg-brand-soft text-ink" : "text-ink2")}
          >
            Introduction
          </NavLink>
        </div>

        {groups.map(({ group, endpoints }) => (
          <div key={group} className="flex flex-col gap-px">
            <div className="px-2.5 pb-[5px] text-[10.5px] font-semibold uppercase tracking-[0.09em] text-ink3">{group}</div>
            {endpoints.map((endpoint) => (
              <NavLink
                key={endpoint.slug}
                to={`/docs/${endpoint.slug}`}
                onClick={onClose}
                className={({ isActive }) => cn(navItemClass, isActive ? "bg-brand-soft text-ink" : "text-ink2")}
              >
                <GetChip />
                <span className="truncate font-mono">{endpointNavLabel(endpoint)}</span>
              </NavLink>
            ))}
          </div>
        ))}

        <div className="flex flex-col gap-px">
          <div className="px-2.5 pb-[5px] text-[10.5px] font-semibold uppercase tracking-[0.09em] text-ink3">More</div>
          <NavLink to="/datasets" onClick={onClose} className={({ isActive }) => cn(navItemClass, isActive ? "bg-brand-soft text-ink" : "text-ink2")}>
            Datasets
          </NavLink>
          <NavLink to="/elections" onClick={onClose} className={({ isActive }) => cn(navItemClass, isActive ? "bg-brand-soft text-ink" : "text-ink2")}>
            Elections
          </NavLink>
          <NavLink to="/docs/demo" onClick={onClose} className={({ isActive }) => cn(navItemClass, isActive ? "bg-brand-soft text-ink" : "text-ink2")}>
            Postal lookup demo
          </NavLink>
        </div>

        {/* Internal SPA route styled as an "exit" per the design (↗ affordance for leaving the docs chrome for the flagship map experience) — kept a real <Link>-style nav so it's a client-side transition, not a full reload. */}
        <NavLink
          to="/map"
          onClick={onClose}
          className="mt-auto flex items-center gap-1.5 rounded-lg border border-border bg-bg3 px-2.5 py-2 text-[13px] font-medium text-ink transition-colors hover:border-brand"
        >
          <MapMark />
          Explore Map
          <span className="ml-auto text-ink3">↗</span>
        </NavLink>
      </nav>

      <StatusRow />
      </aside>
    </>
  );
}
