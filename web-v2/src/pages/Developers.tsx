import { ArrowRight, Bot, Braces, Download, Terminal } from 'lucide-react'
import { Fragment, type ReactNode } from 'react'
import { Link } from 'react-router'
import { Eyebrow, Reveal } from '@/components/common/bits'
import { CodeBlock } from '@/components/common/CodeBlock'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { McpConnect } from '@/features/mcp/McpConnect'
import { CLI_COMMANDS, CLI_INSTALL, CLI_SESSION, DUCKDB, MCP_CALL, MCP_RESULT, REST_ENDPOINTS, REST_REQUEST, REST_RESPONSE } from '@/lib/snippets'
import { SITE_HOST } from '@/lib/site'
import { cn } from '@/lib/utils'

const NAV = [
  { id: 'rest', label: 'REST API', Icon: Braces },
  { id: 'mcp', label: 'MCP server', Icon: Bot },
  { id: 'cli', label: 'lanka CLI', Icon: Terminal },
  { id: 'bulk', label: 'Bulk files', Icon: Download },
]

function Part({ id, n, title, lead, children }: { id: string; n: string; title: string; lead: ReactNode; children: ReactNode }) {
  return (
    <section id={id} className="scroll-mt-28 border-t border-line py-16 sm:py-20">
      <Reveal className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_1.4fr] lg:gap-14">
        <div>
          <Eyebrow n={n}>{id === 'bulk' ? 'downloads' : id}</Eyebrow>
          <h2 className="t-h2 mt-4">{title}</h2>
        </div>
        <p className="t-lead max-w-[58ch] lg:pt-9">{lead}</p>
      </Reveal>
      <div className="mt-10">{children}</div>
    </section>
  )
}

function RefTable({ head, rows }: { head: string[]; rows: ReactNode[][] }) {
  return (
    <div className="scroll-thin overflow-x-auto rounded-xl border border-line bg-bg2">
      <table className="w-full min-w-[560px] border-collapse text-left text-[13.5px]">
        <thead>
          <tr className="border-b border-line">
            {head.map((h) => (
              <th key={h} className="px-4 py-2.5 font-mono text-[11px] font-normal text-ink3">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="border-b border-line align-top last:border-0">
              {r.map((c, j) => (
                <td key={j} className={cn('px-4 py-2.5', j === r.length - 1 ? 'text-ink2' : 'whitespace-nowrap')}>
                  {c}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

const Method = ({ m }: { m: string }) => <span className={cn('inline-block w-10 font-mono text-[11px] font-medium', m === 'GET' ? 'text-ok' : 'text-warn')}>{m}</span>

export default function Developers() {
  return (
    <div className="mx-auto max-w-[1240px] px-5 pt-28 pb-10 sm:px-8 sm:pt-32">
      <header className="grid grid-cols-1 gap-10 pb-14 lg:grid-cols-[1.15fr_1fr] lg:items-end">
        <div>
          <Eyebrow>Developers</Eyebrow>
          <h1 className="t-h1 mt-5">Read-only, anonymous, and the same data everywhere.</h1>
          <p className="t-lead mt-5 max-w-[54ch]">
            A REST API with OpenAPI, an MCP server for models, and a CLI that works offline. No key and no account for anything that reads. Every response says where its numbers came from.
          </p>
          <div className="mt-7 flex flex-wrap gap-2">
            {NAV.map(({ id, label, Icon }) => (
              <a key={id} href={`#${id}`} className="pressable inline-flex h-9 items-center gap-2 rounded-lg border border-line2 bg-bg2 px-3 text-[13px] font-medium text-ink2 hover:border-ink3 hover:text-ink">
                <Icon className="size-3.5" />
                {label}
              </a>
            ))}
          </div>
        </div>
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line">
          {[
            ['Base URL', `${SITE_HOST}/v2`],
            ['MCP endpoint', `${SITE_HOST}/mcp`],
            ['Rate limit', '10 req/s, burst 40'],
            ['Hosting', 'Self-hosted, Rust + Postgres'],
          ].map(([k, v]) => (
            <div key={k} className="bg-bg2 p-4">
              <dt className="text-[12px] text-ink3">{k}</dt>
              {/* an address may wrap after a dot or a slash, never in the middle of a word; <wbr> adds no character to what is copied */}
              <dd className="mt-1.5 font-mono text-[12.5px] [overflow-wrap:anywhere] text-ink">
                {v.split(/(?<=[./])/).map((part, i) => (
                  <Fragment key={i}>
                    {i > 0 && <wbr />}
                    {part}
                  </Fragment>
                ))}
              </dd>
            </div>
          ))}
        </dl>
      </header>

      <Part id="rest" n="01" title="REST API v2" lead="One envelope for every response: success, payload and meta. meta carries the data version, the sources, the licence status and a completeness note, plus ready-made links to the next useful call.">
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[0.8fr_1.2fr]">
          <div className="space-y-4">
            <CodeBlock title="request" code={REST_REQUEST} />
            <RefTable
              head={['', 'Endpoint']}
              rows={REST_ENDPOINTS.map(([m, path, purpose]) => [
                <Method key="m" m={m} />,
                <span key="p" className="whitespace-normal">
                  <code className="text-[12.5px] break-all text-ink">{path}</code>
                  <span className="mt-0.5 block text-[12.5px] text-ink3">{purpose}</span>
                </span>,
              ])}
            />
          </div>
          <CodeBlock title="200 application/json" code={REST_RESPONSE} lang="json" className="self-start lg:sticky lg:top-24" />
        </div>
      </Part>

      <Part id="mcp" n="02" title="MCP server" lead="Connect Claude Code, Codex, Cursor, VS Code or any MCP client and ask about Sri Lanka in plain words: find data, profile an area, compare and rank units, relate two indicators, and get a link that opens the result as a map here. Streamable HTTP with no sign-in, or stdio with lanka mcp.">
        <McpConnect />
        <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
          <CodeBlock title="tools/call" code={MCP_CALL} lang="json" />
          <CodeBlock title="result" code={MCP_RESULT} lang="json" />
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          {['readOnlyHint', 'idempotentHint', 'no destructive tools', '500 rows · 256 KB cap'].map((t) => (
            <Badge key={t} variant="mono">
              {t}
            </Badge>
          ))}
        </div>
      </Part>

      <Part id="cli" n="03" title="The lanka CLI" lead="One binary, offline first. It reads the artifacts you pulled, so it is fast and works without a connection. geocode-csv is the reason most people install it: official p-codes on a spreadsheet of coordinates, in seconds.">
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1.15fr_0.85fr]">
          <CodeBlock title="zsh" code={CLI_SESSION} />
          <div className="space-y-4">
            <CodeBlock title="install" code={CLI_INSTALL} />
            <div className="surface p-5">
              <div className="t-num text-[34px] leading-none font-semibold tracking-[-0.04em]">1.34 M</div>
              <div className="mt-2 text-[13.5px] text-ink2">rows per second through geocode-csv on a laptop. It is one array read per row.</div>
            </div>
          </div>
        </div>
        <div className="mt-4">
          <RefTable head={['Command', 'What it does']} rows={CLI_COMMANDS.map(([cmd, what]) => [<code key="c" className="text-[12.5px] text-ink">{cmd}</code>, what])} />
        </div>
      </Part>

      <Part id="bulk" n="04" title="Bulk files" lead="Every release is a set of immutable, checksummed files: SQLite, Parquet, GeoParquet, PMTiles and cloud-optimised GeoTIFF. Query them in place or take the lot.">
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <CodeBlock title="duckdb" code={DUCKDB} lang="sql" />
          <CodeBlock
            title="release layout"
            lang="text"
            code={`/releases/20261118.1/manifest.json
/releases/20261118.1/lanka-open.sqlite
/releases/20261118.1/parquet/<dataset>.parquet
/releases/20261118.1/geo/<dataset>.{parquet,fgb,geojson.gz}
/releases/20261118.1/grids/<layer>/<period>.tif
/releases/20261118.1/tiles/<layer>.pmtiles
/releases/20261118.1/diffs/<dataset>.parquet
/latest/…`}
          />
        </div>
        <div className="mt-8 flex flex-wrap gap-2.5">
          <Button asChild>
            <Link to="/data">
              Browse datasets <ArrowRight />
            </Link>
          </Button>
          <Button variant="outline" asChild>
            <Link to="/explore">Open the map</Link>
          </Button>
        </div>
      </Part>
    </div>
  )
}
