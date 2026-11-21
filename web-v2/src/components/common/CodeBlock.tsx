import { Check, Copy } from 'lucide-react'
import { Fragment, type ReactNode } from 'react'
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

export function Code({ code, lang = 'bash', className }: { code: string; lang?: Lang; className?: string }) {
  return (
    <pre className={cn('scroll-thin overflow-x-auto font-mono text-[12.5px] leading-[1.7] text-code-ink', className)}>
      <code>
        {highlight(code, lang).map((n, i) => (
          <Fragment key={i}>{n}</Fragment>
        ))}
      </code>
    </pre>
  )
}

/** Code panel. Stays dark in both themes. */
export function CodeBlock({ code, lang = 'bash', title, className, copy = true }: { code: string; lang?: Lang; title?: ReactNode; className?: string; copy?: boolean }) {
  const [copied, doCopy] = useCopy()
  return (
    <div className={cn('group relative overflow-hidden rounded-xl border border-code-line bg-code-bg', className)}>
      {title && (
        <div className="flex h-9 items-center gap-2 border-b border-code-line px-3.5 font-mono text-[11.5px] text-code-ink3">
          {title}
        </div>
      )}
      <Code code={code} lang={lang} className="px-4 py-3.5" />
      {copy && (
        <button
          type="button"
          onClick={() => doCopy(code.replace(/^[$›]\s/gm, ''))}
          aria-label={copied ? 'Copied' : 'Copy code'}
          className={cn(
            'pressable absolute right-2 grid size-7 place-items-center rounded-md border border-code-line bg-code-bg/90 text-code-ink3 opacity-0 backdrop-blur transition-opacity duration-150 group-hover:opacity-100 hover:text-code-ink focus-visible:opacity-100 pointer-coarse:opacity-100',
            title ? 'top-11' : 'top-2',
          )}
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
  )
}
