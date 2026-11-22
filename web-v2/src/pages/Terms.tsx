import { Check, CircleAlert, CircleCheck, CircleHelp, Copy, FileText, Lock, Mail, Scale, ShieldCheck } from 'lucide-react'
import { useEffect, type ReactNode } from 'react'
import { Link, useLocation } from 'react-router'
import { Eyebrow } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { useCopy } from '@/lib/hooks'
import { DATA_CONTACT } from '@/lib/site'
import { cn } from '@/lib/utils'

const UPDATED = '22 November 2026'

const NAV = [
  { id: 'terms', label: 'Terms of use', Icon: FileText },
  { id: 'licensing', label: 'Data licensing', Icon: Scale },
  { id: 'takedown', label: 'Takedown requests', Icon: ShieldCheck },
  { id: 'privacy', label: 'Privacy', Icon: Lock },
]

const TAKEDOWN_SUBJECT = 'Removal request: <dataset id>'
const TAKEDOWN_BODY = `Dataset id or URL:
Your name and organisation:
Your rights or standing (for example: publisher of the source, copyright holder, person concerned):
What you would like us to do (remove, restrict to a link, correct the attribution):
How to reach you:
`
const MAILTO = `mailto:${DATA_CONTACT}?subject=${encodeURIComponent(TAKEDOWN_SUBJECT)}&body=${encodeURIComponent(TAKEDOWN_BODY)}`

function Part({ id, n, title, children }: { id: string; n: string; title: string; children: ReactNode }) {
  return (
    <section id={id} className="scroll-mt-28 border-t border-line py-14 sm:py-16">
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_1.6fr] lg:gap-14">
        <div>
          <Eyebrow n={n}>{id}</Eyebrow>
          <h2 className="t-h2 mt-4">{title}</h2>
        </div>
        <div className="min-w-0 space-y-5 text-[15px] leading-relaxed text-ink2 [&_strong]:font-medium [&_strong]:text-ink">{children}</div>
      </div>
    </section>
  )
}

function Bullets({ items }: { items: ReactNode[] }) {
  return (
    <ul className="space-y-2.5">
      {items.map((item, i) => (
        <li key={i} className="grid grid-cols-[14px_1fr] gap-2.5">
          <span className="mt-[11px] h-px w-3.5 bg-ink3" />
          <span>{item}</span>
        </li>
      ))}
    </ul>
  )
}

const STATUSES: { id: string; label: string; cls: string; Icon: typeof CircleCheck; meaning: ReactNode; reuse: ReactNode }[] = [
  {
    id: 'open',
    label: 'Open',
    cls: 'text-ok bg-ok-soft',
    Icon: CircleCheck,
    meaning: 'Public domain or CC0. The publisher attaches no conditions.',
    reuse: 'Reuse freely. Naming the source is still good practice.',
  },
  {
    id: 'attribution',
    label: 'Open with attribution',
    cls: 'text-ok bg-ok-soft',
    Icon: CircleCheck,
    meaning: 'An open licence that asks for credit, and sometimes share-alike: CC BY, CC BY-IGO, ODbL, CC BY-SA and similar.',
    reuse: 'Reuse freely, credit the source as its licence asks, and keep share-alike data (such as OpenStreetMap under ODbL) under the same licence.',
  },
  {
    id: 'permission_unclear',
    label: 'Permission unclear',
    cls: 'text-warn bg-warn-soft',
    Icon: CircleHelp,
    meaning: 'A public government publication that states no reuse licence. Most Sri Lankan official statistics are in this group.',
    reuse: 'We republish it in the public interest with full attribution while we ask the publisher for written permission. It is not packed into aggregate bundles. Check the publisher’s terms before commercial reuse.',
  },
  {
    id: 'restricted',
    label: 'Restricted',
    cls: 'text-bad bg-bad-soft',
    Icon: CircleAlert,
    meaning: 'The publisher allows non-commercial use only, or forbids redistribution.',
    reuse: 'We do not redistribute the data. We describe it and link to the publisher, who decides how it may be used.',
  },
]

function StatusTable() {
  return (
    <div className="overflow-hidden rounded-xl border border-line bg-bg2">
      {STATUSES.map((s) => (
        <div key={s.id} className="grid grid-cols-1 gap-x-5 gap-y-2 border-b border-line px-4 py-4 last:border-0 sm:grid-cols-[190px_1fr]">
          <div className="flex flex-wrap items-center gap-2 sm:flex-col sm:items-start">
            <span className={cn('inline-flex items-center gap-1.5 rounded-full py-0.5 pr-2 pl-1.5 text-[11.5px] font-medium whitespace-nowrap', s.cls)}>
              <s.Icon className="size-3.5" />
              {s.label}
            </span>
            <code className="font-mono text-[11px] text-ink3">{s.id}</code>
          </div>
          <div className="space-y-1.5 text-[14px] leading-relaxed">
            <p className="text-ink">{s.meaning}</p>
            <p className="text-ink2">{s.reuse}</p>
          </div>
        </div>
      ))}
    </div>
  )
}

const STEPS: [string, string, ReactNode][] = [
  ['01', 'Acknowledge', 'We reply within 2 business days with a reference number.'],
  ['02', 'Disable', 'While we review, we turn off public access to the dataset: its pages, API responses and downloads.'],
  ['03', 'Decide', 'We restore it, keep only a link to the source, or remove it for good, and tell you which and why.'],
  ['04', 'Cascade', 'Anything we derived from a removed dataset is rebuilt without it or taken down with it.'],
]

function ContactCard() {
  const [copied, copy] = useCopy()
  return (
    <div className="surface p-5">
      <div className="t-label">Send removal requests to</div>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <a href={`mailto:${DATA_CONTACT}`} className="font-mono text-[15px] [overflow-wrap:anywhere] text-ink hover:text-brand-ink">
          {DATA_CONTACT}
        </a>
        <button type="button" onClick={() => copy(DATA_CONTACT)} className="pressable inline-flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[12px] text-ink2 hover:bg-bg3 hover:text-ink">
          {copied ? <Check className="size-3.5 text-ok" /> : <Copy className="size-3.5" />}
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <div className="mt-4 flex flex-wrap gap-2.5">
        <Button asChild>
          <a href={MAILTO}>
            <Mail /> Write a request
          </a>
        </Button>
        <Button variant="outline" asChild>
          <Link to="/data">Find the dataset id</Link>
        </Button>
      </div>
      <p className="mt-3.5 text-[12.5px] leading-relaxed text-ink3">The button opens your mail app with the fields below already laid out.</p>
    </div>
  )
}

export default function Terms() {
  const { hash } = useLocation()
  // the page is loaded lazily, so the shell's scroll-to-hash can run before this section exists
  useEffect(() => {
    if (hash) document.getElementById(hash.slice(1))?.scrollIntoView()
  }, [hash])

  return (
    <div className="mx-auto max-w-[1240px] px-5 pt-28 pb-10 sm:px-8 sm:pt-32">
      <header className="grid grid-cols-1 gap-10 pb-14 lg:grid-cols-[1.15fr_1fr] lg:items-end">
        <div>
          <Eyebrow>Terms and licensing</Eyebrow>
          <h1 className="t-h1 mt-5">Every number credited, and removed when its owner asks.</h1>
          <p className="t-lead mt-5 max-w-[56ch]">
            Lanka Data Layer republishes public data about Sri Lanka with its source and licence attached. Where a publisher has not said how its data may be reused, we say so, ask for permission in writing, and take the data down promptly if a rights holder asks.
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
            ['Code', 'MIT licence'],
            ['Data', 'Each source’s own licence'],
            ['Removal requests', 'Acknowledged in 2 business days'],
            ['Last updated', UPDATED],
          ].map(([k, v]) => (
            <div key={k} className="bg-bg2 p-4">
              <dt className="text-[12px] text-ink3">{k}</dt>
              <dd className="mt-1.5 text-[14px] leading-snug font-medium text-ink">{v}</dd>
            </div>
          ))}
        </dl>
      </header>

      <Part id="terms" n="01" title="Terms of use">
        <p>
          Lanka Data Layer is an open data platform. The website, the REST API, the MCP server and the bulk files are free to use, need no account, and are open to anyone. By using them you agree to these terms.
        </p>
        <Bullets
          items={[
            <>
              <strong>Provided as is.</strong> We work to keep the data accurate and current, but it comes without warranty of any kind, including accuracy, completeness or fitness for a particular purpose. Official figures are reproduced as published, errors included; each dataset page says what it cannot tell you. Check anything consequential against the original source.
            </>,
            <>
              <strong>No liability.</strong> To the extent the law allows, we are not liable for losses arising from use of the platform or the data, or from it being unavailable.
            </>,
            <>
              <strong>Credit the source when you reuse data.</strong> Name the original publisher, as given on the dataset page and in every API response, and mention Lanka Data Layer as the place you got it. Follow the source licence’s own terms where it has stricter ones.
            </>,
            <>
              <strong>Use it fairly.</strong> Stay within the published rate limits and do not try to disrupt the service or get around them. Bulk files exist so that you do not have to crawl the API.
            </>,
            <>
              <strong>Not an official source.</strong> We are independent and not affiliated with the Government of Sri Lanka or any publisher we draw on. Inclusion of a dataset does not mean its publisher endorses this platform.
            </>,
          ]}
        />
        <div className="surface p-5">
          <div className="t-label">What we license ourselves</div>
          <ul className="mt-3 space-y-2 text-[14px]">
            <li>
              <strong>Code</strong> is released under the MIT licence.
            </li>
            <li>
              <strong>Data we relay</strong> stays under its source’s licence: we cannot grant more than we received.
            </li>
            <li>
              <strong>Data we derive</strong> takes the strictest status of its inputs. Where every input is openly licensed it is offered under CC BY 4.0; where it redistributes OpenStreetMap geometry, under ODbL 1.0.
            </li>
          </ul>
        </div>
        <p>We may update these terms as the platform changes. The date at the top shows the latest revision.</p>
      </Part>

      <Part id="licensing" n="02" title="Data licensing and attribution">
        <p>
          Every dataset carries its sources and their licences, from the registry through to the page you read and the API response you receive. Each source has one of four licence statuses. A dataset shows the strictest status among its sources, and a derived dataset the strictest among its inputs.
        </p>
        <StatusTable />
        <div className="rounded-xl border border-[color-mix(in_oklab,var(--warn)_35%,var(--line))] bg-warn-soft p-5">
          <div className="flex items-center gap-2 text-[13px] font-medium text-warn">
            <CircleHelp className="size-4" /> About “permission unclear”
          </div>
          <p className="mt-3 text-[14.5px] leading-relaxed text-ink">
            Many Sri Lankan government publications, including census tables, central bank statistics and ministry reports, are published without any stated reuse licence. We republish them because they are public records of public interest, always with the publisher’s name and a link to the original. We are writing to each publisher to ask for permission to reuse their data, and we will remove a dataset if the rights holder asks us to.
          </p>
        </div>
        <p>
          Attribution travels with the data: the dataset page lists every source document, the API’s <code className="font-mono text-[13px] text-ink">meta</code> names the sources and licences behind each response, and every download keeps its dataset’s licence. The licence label on a dataset page is the quickest way to see what applies.
        </p>
      </Part>

      <Part id="takedown" n="03" title="Takedown and removal requests">
        <p>
          If you hold rights in data we publish, or the data concerns you, ask us to remove it. Publishers, copyright and database-right holders, and people affected by a dataset can all make a request.
        </p>
        <ContactCard />
        <div>
          <div className="t-label mb-3">Include in your request</div>
          <Bullets
            items={[
              <>
                <strong>The dataset id or URL</strong>, for example <code className="font-mono text-[13px] text-ink">dcs-census-2024-population</code> or the dataset page’s address.
              </>,
              <>
                <strong>Your rights basis</strong>: why you can make the request, such as being the publisher of the source or the copyright holder.
              </>,
              <>
                <strong>What you want</strong>: removal, a link to the source only, or a corrected attribution.
              </>,
              <>
                <strong>How to contact you</strong>, with your name and organisation.
              </>,
            ]}
          />
        </div>
        <div>
          <div className="t-label mb-3">What we commit to</div>
          <ol className="grid grid-cols-1 gap-px overflow-hidden rounded-xl border border-line bg-line sm:grid-cols-2">
            {STEPS.map(([n, title, body]) => (
              <li key={n} className="grid grid-cols-[26px_1fr] gap-2 bg-bg2 p-4 text-[14px] leading-relaxed">
                <span className="t-num font-mono text-[11.5px] text-ink3">{n}</span>
                <span>
                  <span className="block font-medium text-ink">{title}</span>
                  <span className="mt-1 block text-ink2">{body}</span>
                </span>
              </li>
            ))}
          </ol>
        </div>
        <p>
          Personal data is handled first: anything that appears to identify a private individual is taken offline as soon as we see the request. Copies that others downloaded before removal are outside our control, and backups expire on their normal schedule.
        </p>
      </Part>

      <Part id="privacy" n="04" title="Privacy">
        <p>
          You do not need an account, and the site has no sign-in, analytics, advertising or tracking scripts. The API asks for no key or personal details.
        </p>
        <Bullets
          items={[
            <>
              <strong>Stored in your browser.</strong> Your theme, the map workspace’s layout, unsaved map drafts and the maps you save are kept in your browser’s local storage. They never leave your device unless you share a map, and clearing this site’s data removes them.
            </>,
            <>
              <strong>Shared maps.</strong> When you share a map, its definition (layers, view and settings) is stored on our server so the link works. Anyone with the link can open it.
            </>,
            <>
              <strong>Map tiles.</strong> The basemap and its labels load from OpenFreeMap, which receives the usual request details such as your IP address.
            </>,
            <>
              <strong>Server logs.</strong> Like any web server, ours may record standard request details (IP address, requested address, time) to run the service, enforce rate limits and investigate abuse. We do not sell them or use them to profile visitors.
            </>,
          ]}
        />
        <p>
          Questions about privacy or these terms go to the same address:{' '}
          <a href={`mailto:${DATA_CONTACT}`} className="link-draw text-ink">
            {DATA_CONTACT}
          </a>
          .
        </p>
      </Part>
    </div>
  )
}
