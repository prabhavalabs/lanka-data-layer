import * as React from "react";
import { Outlet, useLocation } from "react-router-dom";

import { Sidebar } from "@/components/layout/sidebar";

function MenuIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true">
      <path d="M2.5 5h13M2.5 9h13M2.5 13h13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

/**
 * The one persistent shell for every route — the design's docs sidebar plus
 * a content slot. No top header bar on desktop (the design has none): each
 * page manages its own scroll container, matching the design's per-screen
 * `<main style="overflow-y:auto">` pattern instead of one shared scroller.
 * Below `md`, the sidebar collapses into an off-canvas sheet (scope's
 * responsive rule) opened by a small mobile-only top bar.
 */
export function AppShell() {
  const [mobileOpen, setMobileOpen] = React.useState(false);
  const location = useLocation();

  // Auto-close the sheet on any navigation (covers browser back/forward and
  // programmatic navigate() calls that NavLink's own onClick wouldn't catch).
  React.useEffect(() => {
    setMobileOpen(false);
  }, [location.pathname]);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground md:flex-row">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 md:hidden">
        <button
          type="button"
          onClick={() => setMobileOpen(true)}
          aria-label="Open navigation"
          className="rounded-md p-1.5 text-ink2 hover:bg-bg3 hover:text-ink"
        >
          <MenuIcon />
        </button>
        <span className="text-[14px] font-semibold tracking-tight">Geopub</span>
      </div>

      <Sidebar mobileOpen={mobileOpen} onClose={() => setMobileOpen(false)} />

      <main className="relative min-h-0 min-w-0 flex-1">
        <Outlet />
      </main>
    </div>
  );
}
