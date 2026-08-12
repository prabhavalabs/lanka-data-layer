export function ElectionsPage() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
      <h2 className="text-[18px] font-semibold tracking-[-0.01em]">Elections</h2>
      <p className="max-w-md text-[13.5px] leading-[1.55] text-ink2">
        The election atlas — results, swing analysis, and turnout maps — lands once the elections endpoints ship in the map UI. In
        the meantime, results are already live at{" "}
        <code className="rounded bg-bg3 px-1.5 py-0.5 font-mono text-[12px] text-ink">/v1/elections</code>.
      </p>
    </div>
  );
}
