import { Link, useParams } from "react-router-dom";

import { Separator } from "@/components/ui/separator";
import { getDocsEndpoint, resolveExampleUrl } from "@/lib/docs-spec";
import { ParamTable } from "@/pages/docs/components/param-table";
import { Playground } from "@/pages/docs/components/playground";

const RESPONSE_KIND_LABEL = {
  envelope: "{success, message, payload, meta} envelope",
  geojson: "bare GeoJSON Feature (not the usual envelope)",
  binary: "raw bytes (not JSON)",
} as const;

export function DocsEndpointPage() {
  const { slug } = useParams<{ slug: string }>();
  const endpoint = slug ? getDocsEndpoint(slug) : undefined;

  if (!endpoint) {
    return (
      <div className="mx-auto flex max-w-3xl flex-col gap-3 p-6">
        <h1 className="text-lg font-semibold">Endpoint not found</h1>
        <p className="text-sm text-muted-foreground">
          There&apos;s no docs page for &quot;{slug}&quot;.{" "}
          <Link to="/docs" className="font-medium text-primary hover:underline">
            Back to the overview
          </Link>
          .
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 p-6">
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2 font-mono text-sm">
          <span className="rounded bg-accent px-1.5 py-0.5 font-semibold text-accent-foreground">{endpoint.method}</span>
          <span className="text-foreground">{endpoint.pathTemplate}</span>
        </div>
        <h1 className="text-xl font-semibold">{endpoint.summary}</h1>
        <p className="text-sm text-muted-foreground">{endpoint.description}</p>
        <p className="text-xs text-muted-foreground">
          Response: <span className="font-mono">{RESPONSE_KIND_LABEL[endpoint.responseKind]}</span>
        </p>
      </div>

      <Separator />

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Parameters</h2>
        <ParamTable params={endpoint.params} />
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Example request</h2>
        <code className="block overflow-x-auto whitespace-nowrap rounded-md border border-border bg-muted/30 px-2.5 py-1.5 font-mono text-xs">
          GET {resolveExampleUrl(endpoint)}
        </code>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Example response</h2>
        <pre className="overflow-x-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs leading-relaxed">
          {endpoint.exampleResponse}
        </pre>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Try it</h2>
        <Playground endpoint={endpoint} />
      </section>

      {endpoint.notes.length > 0 && (
        <section className="flex flex-col gap-2">
          <h2 className="text-sm font-semibold">Notes</h2>
          <ul className="flex flex-col gap-1.5 text-sm text-muted-foreground">
            {endpoint.notes.map((note, i) => (
              <li key={i} className="flex gap-2">
                <span className="select-none text-muted-foreground/60">–</span>
                <span>{note}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
