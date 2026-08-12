import { ThemeToggle } from "@/components/layout/theme-toggle";

export function Header() {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-4">
      <h1 className="text-sm font-medium text-muted-foreground">Geopub Visualization Platform</h1>
      <ThemeToggle />
    </header>
  );
}
