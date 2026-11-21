import { Link } from 'react-router'
import { Logo } from '@/components/common/bits'
import { FooterSkyline } from './FooterSkyline'

const COLS: { h: string; links: [string, string][] }[] = [
  { h: 'Product', links: [['Explore map', '/explore'], ['Area profiles', '/area/LK11'], ['Data catalogue', '/data'], ['Indicators', '/explore']] },
  { h: 'Developers', links: [['REST API v2', '/developers#rest'], ['MCP server', '/developers#mcp'], ['lanka CLI', '/developers#cli'], ['Bulk downloads', '/data']] },
  { h: 'Project', links: [['Method', '/developers'], ['Licences', '/data'], ['Status', '/data'], ['Releases', '/data']] },
]

export function Footer() {
  return (
    <footer className="relative">
      <FooterSkyline />
      <div className="bg-bg2/40">
      <div className="mx-auto max-w-[1240px] px-5 pt-9 pb-10 sm:px-8 sm:pt-12">
        <div className="grid grid-cols-1 gap-10 md:grid-cols-[1.3fr_2fr]">
          <div>
            <Logo />
            <p className="mt-4 max-w-[34ch] text-sm text-ink2">Every Sri Lankan fact with a location, a source and a download. Self-hosted, open infrastructure.</p>
            <p className="mt-5 flex flex-wrap gap-x-4 gap-y-1 text-[15px] text-ink2">
              <span lang="si">ලංකා දත්ත ස්තරය</span>
              <span className="text-line2">/</span>
              <span lang="ta">லங்கா தரவு அடுக்கு</span>
            </p>
          </div>
          <div className="grid grid-cols-2 gap-8 sm:grid-cols-3">
            {COLS.map((c) => (
              <div key={c.h}>
                <div className="t-label mb-3">{c.h}</div>
                <ul className="space-y-2">
                  {c.links.map(([label, to]) => (
                    <li key={label}>
                      <Link to={to} className="link-draw text-sm text-ink2 hover:text-ink">
                        {label}
                      </Link>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </div>
        <div className="mt-12 flex flex-col gap-3 border-t border-line pt-5 font-mono text-[11px] text-ink3 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
            <span className="inline-flex items-center gap-1.5">
              <span className="size-1.5 rounded-full bg-ok" /> all sources fresh
            </span>
            <span>data_version 20261118.1</span>
            <span>7.8731°N 80.7718°E</span>
          </div>
          <div className="flex flex-wrap gap-x-4 gap-y-1">
            <span className="rounded border border-line2 px-1.5 py-px text-warn">design mockup · values are artificial</span>
            <span>Code MIT · data under each source's licence</span>
          </div>
        </div>
      </div>
      </div>
    </footer>
  )
}
