import * as React from "react";
import type { AdminUnit } from "@lanka-data-layer/shared";

import { ACCENT, MONO_FONT, TEXT, TEXT2, TEXT3 } from "@/components/explore/glass";
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
import { CountUp, MicroHeading, StatBar } from "@/components/explore/selection-widgets";
import type {
  AdminSelectionData,
  CoordinateSelectionData,
  PlaceSelectionData,
  PostalSelectionData,
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

interface AdminChain {
  province: AdminUnit | null;
  district: AdminUnit | null;
  ds_division: AdminUnit | null;
  gnd: AdminUnit | null;
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

// --- admin -------------------------------------------------------------------

function buildAdminSections(data: AdminSelectionData): SectionEntry[] {
  const { unit, level, parentChain, children, population, stats } = data;
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
