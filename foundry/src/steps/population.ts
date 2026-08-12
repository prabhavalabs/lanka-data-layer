import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";

export const name = "population";

interface AgeBucket {
  female: number;
  male: number;
  total: number;
}
interface PopUnit {
  pcode: string;
  total: number;
  female: number;
  male: number;
  age: Record<string, AgeBucket>;
}
interface PopulationSource {
  year: number;
  ageBuckets: string[];
  country: Omit<PopUnit, "pcode">;
  provinces: Record<string, PopUnit>;
  districts: Record<string, PopUnit>;
}
interface EthnicityReligionSource {
  year: number;
  ethnicity: Record<string, Record<string, number>>;
  religion: Record<string, Record<string, number>>;
}

/** "00_04" -> "0-4", "75_79" -> "75-79", "80Plus" -> "80+" (per docs/architecture.md §2 age_bucket domain). */
function toContractBucket(sourceBucket: string): string {
  if (sourceBucket === "80Plus") return "80+";
  const m = /^(\d{2})_(\d{2})$/.exec(sourceBucket);
  if (!m) throw new Error(`population: unrecognized age bucket "${sourceBucket}"`);
  return `${Number(m[1])}-${Number(m[2])}`;
}

async function readJson<T>(relPath: string): Promise<T> {
  return JSON.parse(await readFile(rawPath(relPath), "utf8")) as T;
}

/**
 * admin_population: for country + provinces + districts, one row per
 * (age bucket x sex f/m), plus a single sex='t' age_bucket='total' row
 * carrying the unit's overall population. admin_stats: per-district
 * ethnicity/religion counts from the 2012 census, keyed `ethnicity.<name>`
 * / `religion.<name>`.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const pop = await readJson<PopulationSource>("data/population-2023.json");
  const ethRel = await readJson<EthnicityReligionSource>("data/ethnicity-religion-2012.json");

  const insertPop = db.prepare(`
    INSERT INTO admin_population (pcode, year, sex, age_bucket, count)
    VALUES (@pcode, @year, @sex, @age_bucket, @count)
    ON CONFLICT(pcode, year, sex, age_bucket) DO UPDATE SET count=excluded.count
  `);
  const insertStat = db.prepare(`
    INSERT INTO admin_stats (pcode, year, key, value)
    VALUES (@pcode, @year, @key, @value)
    ON CONFLICT(pcode, year, key) DO UPDATE SET value=excluded.value
  `);

  let popRows = 0;
  let statRows = 0;

  const insertUnitPopulation = (pcode: string, unit: Omit<PopUnit, "pcode">) => {
    for (const [sourceBucket, counts] of Object.entries(unit.age)) {
      const age_bucket = toContractBucket(sourceBucket);
      insertPop.run({ pcode, year: pop.year, sex: "f", age_bucket, count: counts.female });
      insertPop.run({ pcode, year: pop.year, sex: "m", age_bucket, count: counts.male });
      popRows += 2;
    }
    insertPop.run({ pcode, year: pop.year, sex: "t", age_bucket: "total", count: unit.total });
    popRows += 1;
  };

  const tx = db.transaction(() => {
    insertUnitPopulation("LK", pop.country);
    for (const unit of Object.values(pop.provinces)) insertUnitPopulation(unit.pcode, unit);
    for (const unit of Object.values(pop.districts)) insertUnitPopulation(unit.pcode, unit);

    for (const [districtName, counts] of Object.entries(ethRel.ethnicity)) {
      const pcode = pop.districts[districtName]?.pcode;
      if (!pcode) throw new Error(`population: no pcode for ethnicity district "${districtName}"`);
      for (const [group, value] of Object.entries(counts)) {
        insertStat.run({ pcode, year: ethRel.year, key: `ethnicity.${group}`, value });
        statRows++;
      }
    }
    for (const [districtName, counts] of Object.entries(ethRel.religion)) {
      const pcode = pop.districts[districtName]?.pcode;
      if (!pcode) throw new Error(`population: no pcode for religion district "${districtName}"`);
      for (const [group, value] of Object.entries(counts)) {
        insertStat.run({ pcode, year: ethRel.year, key: `religion.${group}`, value });
        statRows++;
      }
    }
  });
  tx();

  log(`population: upserted ${popRows} admin_population rows, ${statRows} admin_stats rows`);
}
