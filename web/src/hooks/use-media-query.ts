import * as React from "react";

/**
 * Reactive `window.matchMedia(query).matches` — unlike a one-shot
 * `useMemo(() => matchMedia(...).matches, [])`, this re-renders when the
 * query's answer changes live: a real viewport resize (mobile breakpoint),
 * or a devtools "emulate CSS media" toggle for `prefers-reduced-motion`
 * during manual verification. SSR-safe (`window` guarded) even though this
 * app is client-only, since it's cheap and avoids a hydration foot-gun if
 * that ever changes.
 */
export function useMediaQuery(query: string): boolean {
  const getSnapshot = React.useCallback(() => {
    if (typeof window === "undefined") return false;
    return window.matchMedia(query).matches;
  }, [query]);

  const subscribe = React.useCallback(
    (onChange: () => void) => {
      if (typeof window === "undefined") return () => undefined;
      const mql = window.matchMedia(query);
      mql.addEventListener("change", onChange);
      return () => mql.removeEventListener("change", onChange);
    },
    [query]
  );

  return React.useSyncExternalStore(subscribe, getSnapshot, () => false);
}

/** `(max-width: 767px)` — the breakpoint SelectionCard switches to its mobile bottom-sheet layout at. */
export function useIsMobileViewport(): boolean {
  return useMediaQuery("(max-width: 767px)");
}

/** `(prefers-reduced-motion: reduce)` — gates every animation SelectionCard runs (entrance, stagger, count-up, collapse). */
export function usePrefersReducedMotion(): boolean {
  return useMediaQuery("(prefers-reduced-motion: reduce)");
}
