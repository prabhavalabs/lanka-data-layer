import * as React from "react";

const STORAGE_KEY = "lanka.docs.try-width";
export const TRY_PANEL_DEFAULT_WIDTH = 400;
const MIN_WIDTH = 340;
// Never let the panel swallow the docs column entirely.
const MAX_WIDTH_VIEWPORT_FRACTION = 0.6;

function clampWidth(width: number): number {
  const max = Math.max(MIN_WIDTH, Math.round(window.innerWidth * MAX_WIDTH_VIEWPORT_FRACTION));
  return Math.min(max, Math.max(MIN_WIDTH, Math.round(width)));
}

/**
 * Width state for the resizable Try panel: drag the panel's left edge to
 * resize, double-click the handle to reset. Persists to localStorage so the
 * chosen width follows the reader across endpoint pages and reloads.
 */
export function useTryPanelWidth() {
  const [width, setWidth] = React.useState<number>(() => {
    const stored = Number(window.localStorage.getItem(STORAGE_KEY));
    return Number.isFinite(stored) && stored >= MIN_WIDTH ? clampWidth(stored) : TRY_PANEL_DEFAULT_WIDTH;
  });
  const [dragging, setDragging] = React.useState(false);

  const persist = React.useCallback((value: number) => {
    window.localStorage.setItem(STORAGE_KEY, String(value));
  }, []);

  const onHandlePointerDown = React.useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      const startX = e.clientX;
      const startWidth = width;
      setDragging(true);

      const onMove = (ev: PointerEvent) => {
        // Handle sits on the panel's LEFT edge: dragging left grows the panel.
        setWidth(clampWidth(startWidth + (startX - ev.clientX)));
      };
      const onUp = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        setDragging(false);
        persist(clampWidth(startWidth + (startX - ev.clientX)));
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [width, persist],
  );

  const reset = React.useCallback(() => {
    setWidth(TRY_PANEL_DEFAULT_WIDTH);
    persist(TRY_PANEL_DEFAULT_WIDTH);
  }, [persist]);

  return { width, dragging, onHandlePointerDown, reset };
}
