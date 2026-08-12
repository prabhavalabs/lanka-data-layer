import { NavLink, Outlet } from "react-router-dom";

import { cn } from "@/lib/utils";
import { docsEndpointsByGroup } from "@/lib/docs-spec";

const navLinkClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    "rounded-md px-2.5 py-1.5 text-sm transition-colors",
    isActive ? "bg-accent text-accent-foreground font-medium" : "text-muted-foreground hover:bg-muted hover:text-foreground"
  );

/**
 * Layout for everything under /docs — a local endpoint nav (grouped per
 * docs-spec.ts's DOCS_GROUPS) on the left, page content via <Outlet/> on
 * the right. Nested one level inside AppShell's own Sidebar/Header, so this
 * is intentionally a lighter secondary nav, not a copy of it.
 */
export function DocsLayout() {
  const groups = docsEndpointsByGroup();

  return (
    <div className="flex h-full">
      <nav className="hidden w-60 shrink-0 flex-col gap-4 overflow-y-auto border-r border-border p-4 md:flex">
        <div className="flex flex-col gap-1">
          <NavLink to="/docs" end className={navLinkClass}>
            Overview
          </NavLink>
          <NavLink to="/docs/demo" className={navLinkClass}>
            Postal lookup demo
          </NavLink>
        </div>
        {groups.map(({ group, endpoints }) => (
          <div key={group} className="flex flex-col gap-1">
            <span className="px-2.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{group}</span>
            {endpoints.map((endpoint) => (
              <NavLink key={endpoint.slug} to={`/docs/${endpoint.slug}`} className={navLinkClass}>
                <span className="mr-1.5 font-mono text-[10px] text-muted-foreground">{endpoint.method}</span>
                {endpoint.summary}
              </NavLink>
            ))}
          </div>
        ))}
      </nav>
      <div className="min-w-0 flex-1 overflow-y-auto">
        <Outlet />
      </div>
    </div>
  );
}
