import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { useDatasets, type DatasetsState } from "@/hooks/use-datasets";
import type { Dataset } from "@/lib/dataset";
import type { ApiError } from "@/lib/api";

export function DatasetsPage() {
  const state = useDatasets();

  return (
    <div className="h-full overflow-y-auto p-6">
      <div className="mx-auto flex max-w-5xl flex-col gap-4">
        <div>
          <h2 className="text-lg font-semibold">Datasets</h2>
          <p className="text-sm text-muted-foreground">
            Everything published by the Geopub API, with source and license attribution.
          </p>
        </div>
        <Separator />
        <DatasetsBody state={state} />
      </div>
    </div>
  );
}

function DatasetsBody({ state }: { state: DatasetsState }) {
  if (state.status === "loading") return <DatasetsSkeleton />;
  if (state.status === "error") return <DatasetsError error={state.error} />;
  if (state.data.length === 0) return <DatasetsEmpty />;

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {state.data.map((dataset) => (
        <DatasetCard key={dataset.id} dataset={dataset} />
      ))}
    </div>
  );
}

function DatasetCard({ dataset }: { dataset: Dataset }) {
  return (
    <Card>
      <CardHeader>
        <div className="flex items-start justify-between gap-2">
          <CardTitle>{dataset.title}</CardTitle>
          <span className="shrink-0 rounded-full bg-accent px-2 py-0.5 text-xs font-medium text-accent-foreground">
            {dataset.category}
          </span>
        </div>
        {dataset.description && <CardDescription>{dataset.description}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-1 text-sm text-muted-foreground">
        <div>
          License: <span className="text-foreground">{dataset.license}</span>
        </div>
        {dataset.feature_count != null && (
          <div>
            Features: <span className="text-foreground">{dataset.feature_count.toLocaleString()}</span>
          </div>
        )}
      </CardContent>
      <CardFooter>
        <a
          href={dataset.source_url}
          target="_blank"
          rel="noreferrer"
          className="text-sm font-medium text-primary hover:underline"
        >
          {dataset.source_name} ↗
        </a>
      </CardFooter>
    </Card>
  );
}

function DatasetsSkeleton() {
  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {Array.from({ length: 6 }).map((_, i) => (
        <div key={i} className="h-36 animate-pulse rounded-lg border border-border bg-muted/50" />
      ))}
    </div>
  );
}

function DatasetsEmpty() {
  return (
    <div className="flex flex-col items-center gap-1 rounded-lg border border-dashed border-border py-16 text-center">
      <p className="text-sm font-medium">No datasets published yet</p>
      <p className="text-sm text-muted-foreground">
        The API is reachable, but the `datasets` table is empty for this build.
      </p>
    </div>
  );
}

function DatasetsError({ error }: { error: ApiError }) {
  if (error.unreachable) {
    return (
      <div className="flex flex-col items-center gap-1 rounded-lg border border-dashed border-border py-16 text-center">
        <p className="text-sm font-medium">API not running</p>
        <p className="max-w-sm text-sm text-muted-foreground">
          Couldn&apos;t reach the Geopub API at <code className="text-foreground">/v1</code>. Start it
          with <code className="text-foreground">pnpm api run dev</code> and reload.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-1 rounded-lg border border-dashed border-destructive/40 py-16 text-center">
      <p className="text-sm font-medium text-destructive">Couldn&apos;t load datasets</p>
      <p className="max-w-sm text-sm text-muted-foreground">{error.message}</p>
    </div>
  );
}
