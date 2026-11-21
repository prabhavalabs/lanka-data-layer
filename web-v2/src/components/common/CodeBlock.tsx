import { Check, Copy, WrapText } from 'lucide-react'
import { Fragment, useState, type ReactNode } from 'react'
import { useCopy } from '@/lib/hooks'
import { cn } from '@/lib/utils'

type Lang = 'bash' | 'json' | 'http' | 'sql' | 'text'

const RULES: Record<Lang, [RegExp, string][]> = {
  json: [
    [/"(?:[^"\\]|\\.)*"(?=\s*:)/y, 'var(--tok-key)'],
    [/"(?:[^"\\]|\\.)*"/y, 'var(--tok-str)'],
    [/-?\d+(?:\.\d+)?(?:e[+-]?\d+)?/iy, 'var(--tok-num)'],
    [/\b(?:true|false|null)\b/y, 'var(--tok-kw)'],
    [/[{}[\],:]/y, 'var(--code-ink3)'],
  ],
  bash: [
    [/#.*/y, 'var(--tok-com)'],
    [/"(?:[^"\\]|\\.)*"|'[^']*'/y, 'var(--tok-str)'],
    [/(?<=^|\s)--?[a-zA-Z][\w-]*/y, 'var(--tok-key)'],
    [/(?<=^|\n|\|\s|^\$\s)(?:lanka|curl|npx|brew|cargo|claude|duckdb|sh|pnpm)\b/y, 'var(--tok-kw)'],
    [/https?:\/\/[^\s"']+/y, 'var(--tok-str)'],
    [/(?<=\s)\d+(?:[.,]\d+)*(?=\s|$)/y, 'var(--tok-num)'],
    [/^[$›]\s/my, 'var(--code-ink3)'],
  ],
  http: [
    [/^(?:GET|POST|PUT|DELETE)\b/my, 'var(--tok-kw)'],
    [/^[\w-]+(?=:)/my, 'var(--tok-key)'],
    [/\/v2\/[^\s?]+/y, 'var(--code-ink)'],
    [/[?&][\w.]+=/y, 'var(--tok-key)'],
    [/\b\d{3}\b/y, 'var(--tok-num)'],
  ],
  sql: [
    [/--.*/y, 'var(--tok-com)'],
    [/'[^']*'/y, 'var(--tok-str)'],
    [/\b(?:select|from|where|group by|order by|limit|join|on|as|and|or|desc|asc|sum|avg|count)\b/iy, 'var(--tok-kw)'],
    [/\b\d+(?:\.\d+)?\b/y, 'var(--tok-num)'],
  ],
  text: [],
}

function highlight(code: string, lang: Lang): ReactNode[] {
  const rules = RULES[lang]
  const out: ReactNode[] = []
  let plain = ''
  let i = 0
  const flush = () => {
    if (plain) out.push(plain)
    plain = ''
  }
  outer: while (i < code.length) {
    for (const [re, color] of rules) {
      re.lastIndex = i
      const m = re.exec(code)
      if (m && m[0].length) {
        flush()
        out.push(
          <span key={i} style={{ color }}>
            {m[0]}
          </span>,
        )
        i += m[0].length
        continue outer
      }
    }
    plain += code[i++]
  }
  flush()
  return out
}

export function Code({ code, lang = 'bash', className, wrap = false }: { code: string; lang?: Lang; className?: string; wrap?: boolean }) {
  return (
    <pre
      className={cn(
        'scroll-thin font-mono text-[12.5px] leading-[1.7] text-code-ink',
        wrap ? 'overflow-x-hidden break-words whitespace-pre-wrap' : 'overflow-x-auto',
        className,
      )}
    >
      <code>
        {highlight(code, lang).map((n, i) => (
          <Fragment key={i}>{n}</Fragment>
        ))}
      </code>
    </pre>
  )
}

const iconBtn =
  'pressable grid size-6 place-items-center rounded-md text-code-ink3 transition-colors duration-150 hover:bg-code-line hover:text-code-ink focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-code-ink3'

/** Window chrome dots, macOS order: close, minimise, zoom. Decorative. */
function TrafficLights() {
  return (
    <span aria-hidden className="flex shrink-0 gap-1.5">
      <i className="size-2.5 rounded-full bg-[#ff5f57] ring-1 ring-black/10 ring-inset" />
      <i className="size-2.5 rounded-full bg-[#febc2e] ring-1 ring-black/10 ring-inset" />
      <i className="size-2.5 rounded-full bg-[#28c840] ring-1 ring-black/10 ring-inset" />
    </span>
  )
}

/** Code panel with window chrome. Follows the page theme; long lines scroll (scrollbar on hover) or wrap via the toggle. */
export function CodeBlock({
  code,
  lang = 'bash',
  title,
  className,
  copy = true,
  wrap: wrapDefault = false,
}: {
  code: string
  lang?: Lang
  title?: ReactNode
  className?: string
  copy?: boolean
  wrap?: boolean
}) {
  const [copied, doCopy] = useCopy()
  const [wrap, setWrap] = useState(wrapDefault)
  return (
    <div className={cn('relative overflow-hidden rounded-xl border border-code-line bg-code-bg', className)}>
      <div className="flex h-9 items-center gap-3 border-b border-code-line pr-1.5 pl-3.5">
        <TrafficLights />
        <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-code-ink3">{title}</span>
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            onClick={() => setWrap((w) => !w)}
            aria-pressed={wrap}
            aria-label={wrap ? 'Scroll long lines' : 'Wrap long lines'}
            title={wrap ? 'Scroll long lines' : 'Wrap long lines'}
            className={cn(iconBtn, wrap && 'bg-code-line text-code-ink')}
          >
            <WrapText className="size-3.5" />
          </button>
          {copy && (
            <button
              type="button"
              onClick={() => doCopy(code.replace(/^[$›]\s/gm, ''))}
              aria-label={copied ? 'Copied' : 'Copy code'}
              title={copied ? 'Copied' : 'Copy'}
              className={cn(iconBtn, 'relative')}
            >
              <span className={cn('absolute transition-[opacity,filter,transform] duration-200', copied ? 'scale-90 opacity-0 blur-[2px]' : 'opacity-100')}>
                <Copy className="size-3.5" />
              </span>
              <span className={cn('absolute text-[var(--tok-str)] transition-[opacity,filter,transform] duration-200', copied ? 'opacity-100' : 'scale-90 opacity-0 blur-[2px]')}>
                <Check className="size-3.5" />
              </span>
            </button>
          )}
        </div>
      </div>
      <Code code={code} lang={lang} wrap={wrap} className="px-4 py-3.5" />
    </div>
  )
}
