import * as React from "react";
import type { AdminUnit, SourceAttribution } from "@lanka-data-layer/shared";

import { ACCENT, ACCENT2, BORDER, MONO_FONT, TEXT, TEXT2, TEXT3 } from "@/components/explore/glass";
import {
  ADMIN_CHILD_LABELS,
  buildStatSegments,
  formatArea,
  formatCoord,
  formatDensity,
  formatDistance,
  formatNumber,
  populationSourceLabel,
  shortAdminLabel,
} from "@/components/explore/selection-format";
import { CountUp, MicroHeading, ShimmerBlock, StatBar } from "@/components/explore/selection-widgets";
import type {
  AdminChain,
  AdminPostalCode,
  AdminSelectionData,
  AreaContextSecondaryData,
  CoordinateSelectionData,
  GndContextSecondaryData,
  PlaceSelectionData,
  PostalSelectionData,
  SecondaryData,
  SecondarySection,
  SelectionData,
} from "@/hooks/use-selection-data";
import { selectAdminUnit, selectPostalCode } from "@/lib/map-selection";

interface SectionEntry {
  key: string;
  content: React.ReactNode;
}

/** Renders a list of {key, content} sections, each staggered ~40ms behind the last (index-based delay), skipped entirely under prefers-reduced-motion — see index.css's .animate-section-in. */
function StaggeredSections({ sections, reducedMotion }: { sections: SectionEntry[]; reducedMotion: boolean }) {
  return (
    <>
      {sections.map((section, i) => (
        <div
          key={section.key}
          className={reducedMotion ? undefined : "animate-section-in"}
          style={reducedMotion ? undefined : { animationDelay: `${i * 40}ms` }}
        >
          {section.content}
        </div>
      ))}
    </>
  );
}

interface ChainItem {
  pcode: string;
  name: string;
}

function Breadcrumb({ items }: { items: ChainItem[] }) {
  if (items.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-x-1 gap-y-1 text-[12.5px]">
      {items.map((item, i) => (
        <React.Fragment key={item.pcode}>
          {i > 0 && (
            <span aria-hidden="true" style={{ color: TEXT3 }}>
              /
            </span>
          )}
          <button
            type="button"
            onClick={() => selectAdminUnit(item.pcode, item.name)}
            className="rounded px-1 py-0.5 transition-colors hover:bg-[rgba(120,140,160,0.14)]"
            style={{ color: TEXT2 }}
          >
            {item.name}
          </button>
        </React.Fragment>
      ))}
    </div>
  );
}

function LocationLine({ lat, lon }: { lat: number; lon: number }) {
  return (
    <div className="text-[12.5px] tabular-nums" style={{ color: TEXT2, fontFamily: MONO_FONT }}>
      {formatCoord(lat, lon)}
    </div>
  );
}

function chainToItems(chain: AdminChain): ChainItem[] {
  return [chain.province, chain.district, chain.ds_division, chain.gnd]
    .filter((u): u is AdminUnit => u !== null)
    .map((u) => ({ pcode: u.pcode, name: shortAdminLabel(u.name) }));
}

function PostalLine({ code }: { code: string }) {
  return (
    <button
      type="button"
      onClick={() => selectPostalCode(code)}
      className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-[12.5px] tabular-nums transition-colors hover:bg-[rgba(120,140,160,0.14)]"
      style={{ color: ACCENT, fontFamily: MONO_FONT }}
    >
      {code}
    </button>
  );
}

/**
 * Chips for an admin unit's serving postal codes (DS/GN divisions only —
 * `postal_codes` arrives sorted dominant-first per the API contract, so
 * `codes[0]` is always the emphasized one). Share is shown as a subtle %
 * only below 90% confidence — a dominant code above that threshold reads as
 * "this is simply the postal code here", so the number would just be
 * clutter. Each chip is clickable, same pattern as PostalLine above.
 */
function PostalCodeChips({ codes }: { codes: AdminPostalCode[] }) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {codes.map((pc, i) => {
        const dominant = i === 0;
        const showShare = pc.share < 0.9;
        return (
          <button
            key={pc.code}
            type="button"
            onClick={() => selectPostalCode(pc.code)}
            className="inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[12px] tabular-nums transition-colors hover:bg-[rgba(120,140,160,0.14)]"
            style={{
              fontFamily: MONO_FONT,
              borderColor: dominant ? ACCENT : BORDER,
              color: dominant ? ACCENT : TEXT2,
              background: dominant ? "rgba(216,68,107,0.1)" : "transparent",
              fontWeight: dominant ? 600 : 500,
            }}
          >
            {pc.code}
            {showShare && (
              <span className="text-[10px]" style={{ color: dominant ? ACCENT2 : TEXT3 }}>
                {Math.round(pc.share * 100)}%
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/** Nearest-place line shared by the coordinate core section and both secondary sections (admin's AREA CONTEXT, postal's GND context) — name plus a monospaced distance. */
function NearestPlaceLine({ place }: { place: { name: string; distance_m: number } }) {
  return (
    <div className="text-[12.5px]" style={{ color: TEXT2 }}>
      Nearest {place.name}
      <span className="ml-1.5 text-[11px]" style={{ color: TEXT3, fontFamily: MONO_FONT }}>
        {formatDistance(place.distance_m)}
      </span>
    </div>
  );
}

// --- admin -------------------------------------------------------------------

function buildAdminSections(data: AdminSelectionData): SectionEntry[] {
  const { unit, level, parentChain, children, population, stats, postalCodes } = data;
  const sections: SectionEntry[] = [];

  if (population) {
    const total = population.total?.t ?? 0;
    const density = unit.area_km2 && unit.area_km2 > 0 ? total / unit.area_km2 : null;
    sections.push({
      key: "population",
      content: (
        <div>
          <MicroHeading>Population</MicroHeading>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-semibold" style={{ color: TEXT }}>
              <CountUp value={total} format={formatNumber} />
            </span>
            <span className="text-[11px]" style={{ color: TEXT3 }}>
              {populationSourceLabel(population.year)}
            </span>
          </div>
          {density !== null && (
            <div className="mt-1 text-[12px]" style={{ color: TEXT2 }}>
              Density{" "}
              <span style={{ fontFamily: MONO_FONT, color: TEXT }}>
                <CountUp value={density} format={formatDensity} />
              </span>
            </div>
          )}
        </div>
      ),
    });
  }

  if (unit.area_km2 != null) {
    sections.push({
      key: "area",
      content: (
        <div>
          <MicroHeading>Area</MicroHeading>
          <div className="text-[15px]" style={{ color: TEXT, fontFamily: MONO_FONT }}>
            <CountUp value={unit.area_km2} format={formatArea} />
          </div>
        </div>
      ),
    });
  }

  const chainItems: ChainItem[] = parentChain
    .filter((u) => u.level >= 1)
    .map((u) => ({ pcode: u.pcode, name: shortAdminLabel(u.name) }));
  const childLabel = ADMIN_CHILD_LABELS[level];
  if (chainItems.length > 0 || (childLabel && children.length > 0)) {
    sections.push({
      key: "hierarchy",
      content: (
        <div>
          <MicroHeading>Administrative hierarchy</MicroHeading>
          <Breadcrumb items={chainItems} />
          {childLabel && children.length > 0 && (
            <div className="mt-1.5 text-[12px]" style={{ color: TEXT2 }}>
              {children.length} {childLabel.toLowerCase()}
            </div>
          )}
        </div>
      ),
    });
  }

  if (stats) {
    const ethnicity = buildStatSegments(stats.values, "ethnicity");
    const religion = buildStatSegments(stats.values, "religion");
    if (ethnicity.length > 0 || religion.length > 0) {
      sections.push({
        key: "demographics",
        content: (
          <div className="space-y-3">
            <MicroHeading>Demographics</MicroHeading>
            <StatBar title="Ethnicity" segments={ethnicity} />
            <StatBar title="Religion" segments={religion} />
            <div className="text-[10.5px]" style={{ color: TEXT3 }}>
              Census {stats.year}
            </div>
          </div>
        ),
      });
    }
  }

  if (postalCodes.length > 0) {
    sections.push({
      key: "postal-codes",
      content: (
        <div>
          <MicroHeading>Postal codes</MicroHeading>
          <PostalCodeChips codes={postalCodes} />
        </div>
      ),
    });
  }

  if (unit.centroid) {
    sections.push({
      key: "location",
      content: (
        <div>
          <MicroHeading>Location</MicroHeading>
          <LocationLine lat={unit.centroid.lat} lon={unit.centroid.lon} />
        </div>
      ),
    });
  }

  return sections;
}

// --- postal --------------------------------------------------------------------

function buildPostalSections(data: PostalSelectionData): SectionEntry[] {
  const sections: SectionEntry[] = [];
  const chainItems = chainToItems(data);

  if (chainItems.length > 0) {
    sections.push({
      key: "hierarchy",
      content: (
        <div>
          <MicroHeading>Administrative hierarchy</MicroHeading>
          <Breadcrumb items={chainItems} />
        </div>
      ),
    });
  }

  if (data.lat != null && data.lon != null) {
    sections.push({
      key: "location",
      content: (
        <div>
          <MicroHeading>Location</MicroHeading>
          <LocationLine lat={data.lat} lon={data.lon} />
        </div>
      ),
    });
  }

  return sections;
}

// --- place -----------------------------------------------------------------------

function buildPlaceSections(data: PlaceSelectionData): SectionEntry[] {
  const sections: SectionEntry[] = [];
  const { reverse } = data;
  const chainItems = chainToItems(reverse);

  if (chainItems.length > 0) {
    sections.push({
      key: "hierarchy",
      content: (
        <div>
          <MicroHeading>Administrative hierarchy</MicroHeading>
          <Breadcrumb items={chainItems} />
        </div>
      ),
    });
  }

  if (reverse.postal_code) {
    sections.push({
      key: "postal",
      content: (
        <div>
          <MicroHeading>Postal code</MicroHeading>
          <PostalLine code={reverse.postal_code} />
        </div>
      ),
    });
  }

  sections.push({
    key: "location",
    content: (
      <div>
        <MicroHeading>Location</MicroHeading>
        <LocationLine lat={data.lat} lon={data.lon} />
      </div>
    ),
  });

  return sections;
}

// --- coordinate ---------------------------------------------------------------------

function buildCoordinateSections(data: CoordinateSelectionData): SectionEntry[] {
  const sections: SectionEntry[] = [];
  const { reverse } = data;
  const chainItems = chainToItems(reverse);

  if (chainItems.length > 0) {
    sections.push({
      key: "hierarchy",
      content: (
        <div>
          <MicroHeading>Administrative hierarchy</MicroHeading>
          <Breadcrumb items={chainItems} />
        </div>
      ),
    });
  }

  if (reverse.postal_code) {
    sections.push({
      key: "postal",
      content: (
        <div>
          <MicroHeading>Postal code</MicroHeading>
          <PostalLine code={reverse.postal_code} />
        </div>
      ),
    });
  }

  if (reverse.nearest_place) {
    const place = reverse.nearest_place;
    sections.push({
      key: "nearest",
      content: (
        <div>
          <MicroHeading>Nearest place</MicroHeading>
          <div className="text-[13px]" style={{ color: TEXT }}>
            {place.name}
            <span className="ml-1.5 text-[11.5px]" style={{ color: TEXT3, fontFamily: MONO_FONT }}>
              {formatDistance(place.distance_m)} away
            </span>
          </div>
        </div>
      ),
    });
  }

  sections.push({
    key: "location",
    content: (
      <div>
        <MicroHeading>Location</MicroHeading>
        <LocationLine lat={data.lat} lon={data.lon} />
      </div>
    ),
  });

  return sections;
}

export function SelectionSections({ data, reducedMotion }: { data: SelectionData; reducedMotion: boolean }) {
  const sections =
    data.type === "admin"
      ? buildAdminSections(data)
      : data.type === "postal"
        ? buildPostalSections(data)
        : data.type === "place"
          ? buildPlaceSections(data)
          : buildCoordinateSections(data);

  return <StaggeredSections sections={sections} reducedMotion={reducedMotion} />;
}

// --- secondary (progressive enrichment) ---------------------------------------

/** A skeleton standing in for a secondary section while its request is in flight — a heading-shaped bar plus a body-shaped bar, same rhythm as the rest of the card's MicroHeading + content pattern. */
function SecondarySkeleton() {
  return (
    <div className="space-y-1.5">
      <ShimmerBlock className="h-2.5 w-24" />
      <ShimmerBlock className="h-3.5 w-4/5" />
    </div>
  );
}

/** "AREA CONTEXT": population within 5km, plus (admin only) the nearest named place to that point — see AreaContextSecondaryData's doc comment. */
function AreaContextContent({ data }: { data: AreaContextSecondaryData }) {
  return (
    <div className="space-y-1">
      <MicroHeading>Area context</MicroHeading>
      {data.populationRadius !== null && (
        <div className="text-[13px]" style={{ color: TEXT }}>
          ~<CountUp value={data.populationRadius} format={formatNumber} /> people within 5 km
        </div>
      )}
      {data.nearestPlace && <NearestPlaceLine place={data.nearestPlace} />}
    </div>
  );
}

/** Postal's "GND context" fill-in — an administrative hierarchy breadcrumb and/or a nearest-place line, arriving late because the core postal response's own chain came back empty. */
function GndContextContent({ data }: { data: GndContextSecondaryData }) {
  const chainItems = chainToItems(data.chain);
  return (
    <div className="space-y-1">
      {chainItems.length > 0 && (
        <>
          <MicroHeading>Administrative hierarchy</MicroHeading>
          <Breadcrumb items={chainItems} />
        </>
      )}
      {data.nearestPlace && <NearestPlaceLine place={data.nearestPlace} />}
    </div>
  );
}

function renderSecondaryData(data: SecondaryData): React.ReactNode {
  if (data.kind === "area-context") return <AreaContextContent data={data} />;
  return <GndContextContent data={data} />;
}

/** Tiny per-section attribution — "WorldPop · CC BY 4.0" — pulled from that section's own response, distinct from (and feeding into) the card footer's aggregate source list. Multiple contributing sources (e.g. admin's AREA CONTEXT, built from both /population and /reverse) list just their names to stay compact. */
function InlineSourceTag({ source }: { source: SourceAttribution[] }) {
  if (source.length === 0) return null;
  const label = source.length === 1 ? `${source[0].name} · ${source[0].license}` : source.map((s) => s.name).join(" + ");
  return (
    <div className="mt-1 text-[9.5px]" style={{ color: TEXT3 }}>
      {label}
    </div>
  );
}

/**
 * Renders the queue of secondary (progressively-loading) sections below the
 * card's primary sections: each starts as a skeleton (`SecondarySkeleton`)
 * and, once its data lands, fades in via the same `animate-section-in` the
 * primary sections use, with its own inline source tag. A section whose job
 * never resolves to anything (404/error/empty) is simply absent from
 * `sections` (see useSelectionSecondary) and never rendered here — no error
 * state for enrichment data, per the task brief.
 */
export function SecondarySections({ sections, reducedMotion }: { sections: SecondarySection[]; reducedMotion: boolean }) {
  if (sections.length === 0) return null;
  return (
    <>
      {sections.map((section) => (
        <div key={section.key} className={section.status === "ready" && !reducedMotion ? "animate-section-in" : undefined}>
          {section.status === "loading" || !section.data ? (
            <SecondarySkeleton />
          ) : (
            <>
              {renderSecondaryData(section.data)}
              <InlineSourceTag source={section.source} />
            </>
          )}
        </div>
      ))}
    </>
  );
}
