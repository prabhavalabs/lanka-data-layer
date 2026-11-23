// Connect an AI client to the MCP server: the endpoint and what it speaks, one row per client with the
// one action that connects it (a link that installs it, or the command or config to copy), a live
// handshake, and the tools the server actually registers.
import { useQuery } from '@tanstack/react-query'
import { Check, ChevronDown, Copy, ExternalLink, Loader2, PlugZap } from 'lucide-react'
import { useMemo, useState } from 'react'
import { clientSetups, handshake, mcpEndpoint, mcpInfoQuery, type ClientSetup, type Handshake } from '@/api/mcp'
import { CodeBlock } from '@/components/common/CodeBlock'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useCopy } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const LOCAL = /^https?:\/\/(127\.|localhost|0\.0\.0\.0|\[::1\])/

/** `compact`: the dialog in the map workspace (no tool table, tighter rows). */
export function McpConnect({ compact = false }: { compact?: boolean }) {
  const info = useQuery(mcpInfoQuery())
  const url = mcpEndpoint(info.data)
  const local = LOCAL.test(url)
  const setups = useMemo(() => clientSetups(url, info.data?.stdio_command ?? 'lanka mcp'), [url, info.data?.stdio_command])
  // the server lists them oldest first; newest first reads better, and is what a client asks for
  const versions = [...(info.data?.protocol_versions ?? [])].reverse()
  const [test, setTest] = useState<Handshake | 'running' | null>(null)
  const [urlCopied, copyUrl] = useCopy()

  return (
    <div className="space-y-4">
      {/* the endpoint and what it speaks */}
      <div className="rounded-xl border border-line bg-bg2">
        <div className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-3">
          <span className="text-[12px] text-ink3">Endpoint</span>
          <code className="min-w-0 flex-1 truncate font-mono text-[12.5px] text-ink">{url}</code>
          <Button variant="outline" size="sm" onClick={() => copyUrl(url)} className="h-7 gap-1.5 px-2 text-[12px] max-md:h-9">
            {urlCopied ? <Check /> : <Copy />}
            {urlCopied ? 'Copied' : 'Copy'}
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={test === 'running'}
            onClick={async () => {
              setTest('running')
              setTest(await handshake(url, versions[0] ?? '2025-06-18'))
            }}
            className="h-7 gap-1.5 px-2 text-[12px] max-md:h-9"
          >
            {test === 'running' ? <Loader2 className="animate-spin" /> : <PlugZap />}
            Test connection
          </Button>
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2.5 text-[12px] text-ink3">
          <span className="flex items-center gap-1.5">
            Transports
            {(info.data?.transports ?? ['streamable-http', 'stdio']).map((t) => (
              <Badge key={t} variant="mono">
                {t}
              </Badge>
            ))}
          </span>
          <span className="flex flex-wrap items-center gap-1.5">
            Protocol
            {versions.length ? (
              versions.map((v, i) => (
                <Badge key={v} variant={i === 0 ? 'neutral' : 'mono'} title={i === 0 ? 'Latest revision this server speaks' : 'Also negotiated'}>
                  {v}
                </Badge>
              ))
            ) : (
              <span>{info.isLoading ? 'checking…' : 'unknown (the engine is not reachable from here)'}</span>
            )}
          </span>
          {info.data && (
            <span>
              {info.data.tools.length} tools · v{info.data.version}
            </span>
          )}
        </div>
        {test && test !== 'running' && (
          <p role="status" className={cn('border-t border-line px-4 py-2 text-[12.5px]', test.ok ? 'text-ok' : 'text-bad')}>
            {test.ok ? `Connected: ${test.server} ${test.version} agreed on protocol ${test.protocolVersion} in ${test.ms} ms.` : `Could not connect: ${test.error}`}
          </p>
        )}
        {local && (
          <p className="border-t border-line px-4 py-2 text-[12px] leading-[1.5] text-ink3">
            This is a local address: clients on this machine (Claude Code, Codex, Cursor, VS Code) can use it; hosted ones (Claude on the web) need the public endpoint.
          </p>
        )}
      </div>

      {/* one row per client */}
      <div className="overflow-hidden rounded-xl border border-line" role="table" aria-label="Connect a client">
        <div role="row" className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 border-b border-line bg-bg3/50 px-4 py-2 text-[11.5px] font-medium tracking-[0.04em] text-ink3 uppercase">
          <span role="columnheader">Client</span>
          <span role="columnheader">Connect</span>
        </div>
        {setups.map((s) => (
          <ClientRow key={s.id} s={s} compact={compact} disabled={!!s.needsPublic && local} />
        ))}
      </div>

      {/* the tools, as the server registers them */}
      {!compact && info.data && (
        <div className="overflow-hidden rounded-xl border border-line">
          <div className="grid grid-cols-[minmax(140px,0.4fr)_1fr_auto] gap-3 border-b border-line bg-bg3/50 px-4 py-2 text-[11.5px] font-medium tracking-[0.04em] text-ink3 uppercase">
            <span>Tool</span>
            <span>What it does</span>
            <span />
          </div>
          {info.data.tools.map((t) => (
            <div key={t.name} className="grid grid-cols-[minmax(140px,0.4fr)_1fr_auto] items-start gap-3 border-b border-line px-4 py-2.5 last:border-0">
              <code className="font-mono text-[12.5px] break-all text-ink">{t.name}</code>
              <span className="text-[12.5px] leading-[1.5] text-ink2">{t.description}</span>
              <Badge variant={t.read_only ? 'mono' : 'warn'}>{t.read_only ? 'read-only' : 'creates a link'}</Badge>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function ClientRow({ s, compact, disabled }: { s: ClientSetup; compact: boolean; disabled: boolean }) {
  const [copied, copy] = useCopy()
  const [open, setOpen] = useState(false)
  return (
    <div role="row" className="border-b border-line last:border-0">
      <div className={cn('grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-4', compact ? 'py-2' : 'py-2.5')}>
        <div role="cell" className="min-w-0">
          <button type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open} className="group flex min-w-0 items-center gap-1.5 text-left">
            <span className="truncate text-[13px] font-medium text-ink">{s.name}</span>
            <ChevronDown className={cn('size-3.5 shrink-0 text-ink3 transition-transform duration-150', open && 'rotate-180')} />
          </button>
          <span className="block truncate text-[12px] text-ink3">{disabled ? 'Needs the public endpoint, not a local one' : s.where}</span>
        </div>
        <div role="cell" className="flex shrink-0 items-center gap-1.5">
          {s.href && (
            <Button asChild size="sm" variant="primary" className={cn('h-7 gap-1.5 px-2.5 text-[12px] max-md:h-9', disabled && 'pointer-events-none opacity-50')}>
              <a href={s.href}>
                <ExternalLink />
                Add to {s.name}
              </a>
            </Button>
          )}
          <Button size="sm" variant={s.href ? 'outline' : 'primary'} disabled={disabled} onClick={() => copy(s.copy)} className="h-7 w-[118px] gap-1.5 px-2.5 text-[12px] max-md:h-9">
            {copied ? <Check /> : <Copy />}
            {copied ? 'Copied' : s.kind === 'command' ? 'Copy command' : s.kind === 'url' ? 'Copy URL' : 'Copy config'}
          </Button>
        </div>
      </div>
      {open && (
        <div className="px-4 pb-3">
          <CodeBlock code={s.copy} lang={s.lang} />
        </div>
      )}
    </div>
  )
}
