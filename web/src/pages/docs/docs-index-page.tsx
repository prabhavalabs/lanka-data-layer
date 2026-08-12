import { Link } from "react-router-dom";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";

const GITHUB_URL = "https://github.com/prabhavalabs/lanka-data-layer";

const ENVELOPE_EXAMPLE = `{
  "success": true,
  "message": "OK",
  "payload": { /* endpoint-specific */ },
  "meta": {
    "data_version": "20260812.7",
    "source": [
      { "name": "OpenStreetMap", "url": "https://www.openstreetmap.org", "license": "ODbL" }
    ]
  }
}`;

export function DocsIndexPage() {
  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 p-6">
      <div>
        <h1 className="text-xl font-semibold">Geopub API</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          An HTTP service over Sri Lanka&apos;s administrative, postal, population, and election data. Pick an endpoint from the
          nav to read its parameters and try it live, or jump to the{" "}
          <Link to="/docs/demo" className="font-medium text-primary hover:underline">
            postal lookup demo
          </Link>{" "}
          to see the API answer a real query and render it on a map.
        </p>
      </div>

      <Separator />

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Base URL</h2>
        <p className="text-sm text-muted-foreground">
          Every route lives under <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">/v1</code>. In this
          app, requests go through Vite&apos;s dev proxy (or the production nginx vhost) at the current origin — the playgrounds
          on each endpoint page fetch that way. For a standalone request (e.g. via curl), the API listens on{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">http://localhost:8600</code> in
          development, matching{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">api/README.md</code>&apos;s default{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">PORT</code>.
        </p>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Envelope</h2>
        <p className="text-sm text-muted-foreground">
          Every response — success or failure — uses the same shape: <code className="font-mono">success</code>,{" "}
          <code className="font-mono">message</code>, <code className="font-mono">payload</code> (null on failure), and{" "}
          <code className="font-mono">meta</code> (<code className="font-mono">data_version</code> plus{" "}
          <code className="font-mono">source</code> attribution for every dataset the response touched). Two routes are
          deliberate exceptions —{" "}
          <Link to="/docs/admin-geometry" className="font-medium text-primary hover:underline">
            /v1/admin/:pcode/geometry
          </Link>{" "}
          returns a bare GeoJSON Feature, and{" "}
          <Link to="/docs/tiles" className="font-medium text-primary hover:underline">
            /v1/tiles/:file
          </Link>{" "}
          streams raw bytes — both are noted on their own pages.
        </p>
        <pre className="overflow-x-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs">{ENVELOPE_EXAMPLE}</pre>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Language</h2>
        <p className="text-sm text-muted-foreground">
          Most routes accept <code className="font-mono">lang=en|si|ta</code> (default <code className="font-mono">en</code>) to
          select which name field comes back. A missing translation falls back to English rather than erroring or returning
          null — Sinhala and Tamil coverage varies by dataset.
        </p>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Caching</h2>
        <p className="text-sm text-muted-foreground">
          Every <code className="font-mono">GET</code> except <code className="font-mono">/v1/health</code> sets{" "}
          <code className="font-mono">ETag: &quot;&lt;data_version&gt;-&lt;hash&gt;&quot;</code> and{" "}
          <code className="font-mono">Cache-Control: public, max-age=86400, stale-while-revalidate=604800</code>, and honors{" "}
          <code className="font-mono">If-None-Match</code> with a <code className="font-mono">304</code>. Admin geometry and
          tiles strengthen that to <code className="font-mono">immutable</code> — a pcode&apos;s boundary or a build&apos;s
          tiles never change within one <code className="font-mono">data_version</code>. A new data release rotates{" "}
          <code className="font-mono">data_version</code>, which rotates every ETag at once.
        </p>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Attribution</h2>
        <p className="text-sm text-muted-foreground">
          <code className="font-mono">meta.source</code> lists every dataset a response drew from — name, source URL, and
          license — sourced from the{" "}
          <Link to="/docs/datasets" className="font-medium text-primary hover:underline">
            /v1/datasets
          </Link>{" "}
          catalog. Attribution failures are never fatal: an unrecognized dataset id is silently skipped rather than turning an
          otherwise-successful response into a 500.
        </p>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-semibold">Rate limits</h2>
        <p className="text-sm text-muted-foreground">None yet.</p>
      </section>

      <Card>
        <CardHeader>
          <CardTitle>Source</CardTitle>
        </CardHeader>
        <CardContent>
          <a href={GITHUB_URL} target="_blank" rel="noreferrer" className="text-sm font-medium text-primary hover:underline">
            {GITHUB_URL} ↗
          </a>
        </CardContent>
      </Card>
    </div>
  );
}
