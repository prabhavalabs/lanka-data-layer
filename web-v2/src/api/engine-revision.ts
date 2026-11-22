// An analysis id is a fingerprint that includes the method's and the engine's revision, so a result
// cached under an earlier engine revision is not the one the engine would give now. Nothing keeps
// results across sessions (no localStorage of results, no query persistence; documents store requests,
// never result ids), but within a session the engine runs and the query cache would: when a result
// arrives from another engine revision than the last one seen, both are dropped and open queries rerun.
// Kept free of imports so the app shell can wire it without loading the analysis client.
const KEY = 'ldl.analysis.engine_revision'
let seen: number | null = (() => {
  try {
    const v = Number(localStorage.getItem(KEY))
    return Number.isFinite(v) && v > 0 ? v : null
  } catch {
    return null
  }
})()
const listeners = new Set<() => void>()

/** Called when the engine's revision changes from the one last seen (in this browser): drop cached analyses. */
export function onEngineRevisionChange(fn: () => void): () => void {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

/** Records a result's `provenance.engine_revision`; true when it differs from the last one seen (the listeners are told next). */
export function noteEngineRevision(rev: number | undefined): boolean {
  if (rev === undefined || rev === seen) return false
  const changed = seen !== null
  seen = rev
  try {
    localStorage.setItem(KEY, String(rev))
  } catch {
    /* private mode: the in-memory value is enough */
  }
  // after the caller has dropped its own caches
  if (changed) queueMicrotask(() => listeners.forEach((fn) => fn()))
  return changed
}
