import { cn } from "@/lib/utils";

/**
 * Presentational toggle-switch indicator — no click handler, no `<button>`
 * of its own. Nesting an interactive control inside another interactive
 * control (e.g. this inside a `<label>` or row `<button>`) is invalid HTML
 * and causes double-toggle bugs when both fire on click, so callers make
 * the whole row the single interactive element (`role="switch"` +
 * `aria-checked` on that row) and drop this in purely for the visual.
 * See layer-panel.tsx for the intended usage.
 */
export function Switch({ checked, className }: { checked: boolean; className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors",
        checked ? "bg-primary" : "bg-muted",
        className
      )}
    >
      <span
        className={cn(
          "inline-block size-4 translate-x-0.5 transform rounded-full bg-background shadow transition-transform",
          checked && "translate-x-[18px]"
        )}
      />
    </span>
  );
}
