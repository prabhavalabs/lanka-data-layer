import { ThemeToggle } from "@/components/layout/theme-toggle";
import { Omnibox } from "@/components/search/omnibox";

export function Header() {
  return (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border px-4">
      <h1 className="shrink-0 text-sm font-medium text-muted-foreground">Geopub Visualization Platform</h1>
      <div className="flex flex-1 justify-center">
        <Omnibox />
      </div>
      <ThemeToggle />
    </header>
  );
}
