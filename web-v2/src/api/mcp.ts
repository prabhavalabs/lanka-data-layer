// The engine's MCP server, as the web app sees it: what `GET /v2/mcp` says about it (endpoint, transports,
// protocol revisions, tools), and a live handshake against `/mcp` so "Test connection" shows what a client
// would negotiate.
import { queryOptions } from '@tanstack/react-query'
import { absoluteApiUrl, apiAvailable, getJson } from './http'
import { MCP } from '@/lib/site'

export type McpTool = { name: string; title?: string | null; description: string; read_only: boolean }
export type McpInfo = { name: string; title?: string; version: string; endpoint: string; transports: string[]; protocol_versions: string[]; stdio_command: string; tools: McpTool[] }

/** The server's own description; null when the engine is not reachable (the page then shows the public endpoint). */
export const mcpInfoQuery = () =>
  queryOptions({
    queryKey: ['mcp-info'],
    queryFn: async (): Promise<McpInfo | null> => ((await apiAvailable()) ? getJson<McpInfo>('/v2/mcp').catch(() => null) : null),
    staleTime: 5 * 60_000,
    retry: false,
  })

/** The address a client connects to: the engine's own, absolute; the public one when the engine is not here. */
export function mcpEndpoint(info: McpInfo | null | undefined): string {
  if (!info) return MCP
  return /^https?:\/\//.test(info.endpoint) ? info.endpoint : absoluteApiUrl(info.endpoint || '/mcp')
}

export type Handshake = { ok: true; protocolVersion: string; server: string; version: string; ms: number } | { ok: false; error: string }

/** `initialize` over streamable HTTP, the way a client opens a session; the reply may come as JSON or as one SSE event. */
export async function handshake(endpoint: string, protocolVersion: string): Promise<Handshake> {
  const t0 = performance.now()
  try {
    const res = await fetch(endpoint, {
      method: 'POST',
      headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', 'mcp-protocol-version': protocolVersion },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion, capabilities: {}, clientInfo: { name: 'lanka-data-layer-web', version: '1' } } }),
      signal: AbortSignal.timeout(8000),
    })
    if (!res.ok) return { ok: false, error: `${res.status} ${res.statusText}` }
    const text = await res.text()
    const json = (res.headers.get('content-type') ?? '').includes('event-stream')
      ? text
          .split('\n')
          .filter((l) => l.startsWith('data:'))
          .map((l) => l.slice(5).trim())
          .find((l) => l.startsWith('{'))
      : text
    const body = JSON.parse(json ?? 'null') as { result?: { protocolVersion: string; serverInfo: { name: string; version: string } }; error?: { message: string } } | null
    if (!body?.result) return { ok: false, error: body?.error?.message ?? 'No initialize result' }
    // a well-behaved client closes the session it opened
    const session = res.headers.get('mcp-session-id')
    if (session) void fetch(endpoint, { method: 'DELETE', headers: { 'mcp-session-id': session } }).catch(() => {})
    return { ok: true, protocolVersion: body.result.protocolVersion, server: body.result.serverInfo.name, version: body.result.serverInfo.version, ms: Math.round(performance.now() - t0) }
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) }
  }
}

/* ---------------- how each client connects ---------------- */
export const SERVER_NAME = 'lanka-data-layer'
const b64 = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s)))

export type ClientSetup = {
  id: string
  name: string
  /** what the person does: open a link that installs it, run a command, or paste a config */
  kind: 'link' | 'command' | 'config' | 'url'
  /** the one-click install link, for clients that register one */
  href?: string
  /** what is copied (a command, a config block, or the URL) */
  copy: string
  /** where it goes, in a few words */
  where: string
  /** the client has to reach the endpoint from the internet (a local address will not do) */
  needsPublic?: boolean
  lang: 'bash' | 'json' | 'text'
}

export function clientSetups(url: string, stdio: string): ClientSetup[] {
  const http = { type: 'http', url }
  return [
    { id: 'claude-code', name: 'Claude Code', kind: 'command', copy: `claude mcp add --transport http ${SERVER_NAME} ${url}`, where: 'Run in a terminal', lang: 'bash' },
    { id: 'codex', name: 'Codex CLI', kind: 'command', copy: `codex mcp add ${SERVER_NAME} --url ${url}`, where: 'Run in a terminal (or add it under [mcp_servers] in ~/.codex/config.toml)', lang: 'bash' },
    { id: 'cursor', name: 'Cursor', kind: 'link', href: `cursor://anysphere.cursor-deeplink/mcp/install?name=${SERVER_NAME}&config=${encodeURIComponent(b64(JSON.stringify({ url })))}`, copy: JSON.stringify({ mcpServers: { [SERVER_NAME]: { url } } }, null, 2), where: 'Opens Cursor and asks to install', lang: 'json' },
    { id: 'vscode', name: 'VS Code', kind: 'link', href: `vscode:mcp/install?${encodeURIComponent(JSON.stringify({ name: SERVER_NAME, ...http }))}`, copy: JSON.stringify({ servers: { [SERVER_NAME]: http } }, null, 2), where: 'Opens VS Code and asks to install (Copilot agent mode)', lang: 'json' },
    { id: 'gemini', name: 'Gemini CLI', kind: 'command', copy: `gemini mcp add --transport http ${SERVER_NAME} ${url}`, where: 'Run in a terminal', lang: 'bash' },
    { id: 'claude-ai', name: 'Claude (web and desktop)', kind: 'url', copy: url, where: 'Settings → Connectors → Add custom connector, then paste the URL', needsPublic: true, lang: 'text' },
    { id: 'windsurf', name: 'Windsurf', kind: 'config', copy: JSON.stringify({ mcpServers: { [SERVER_NAME]: { serverUrl: url } } }, null, 2), where: 'Paste into ~/.codeium/windsurf/mcp_config.json', lang: 'json' },
    { id: 'json', name: 'Any MCP client', kind: 'config', copy: JSON.stringify({ mcpServers: { [SERVER_NAME]: http } }, null, 2), where: 'Streamable HTTP, no key and no sign-in', lang: 'json' },
    { id: 'stdio', name: 'Local (stdio)', kind: 'config', copy: JSON.stringify({ mcpServers: { [SERVER_NAME]: { command: stdio.split(' ')[0], args: stdio.split(' ').slice(1), env: { DATABASE_URL: 'postgres://…' } } } }, null, 2), where: `Runs ${stdio} on your machine against your own database`, lang: 'json' },
  ]
}
