import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { cellId } from "@lanka-data-layer/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotFoundError, NotInCoverageError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { resolveAdminLevels } from "../lib/admin.ts";
import { findPostalByCode } from "../lib/postalSearch.ts";

const CodeQuerySchema = z.object({
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
});

const PointQuerySchema = z.object({
  lat: z.coerce.number().finite(),
  lon: z.coerce.number().finite(),
});

interface CellLookupPostalRow {
  postal_code: string | null;
}

/**
 * GET /v1/postal/:code and GET /v1/postal?lat&lon (contract §4).
 *
 * `postal_codes.admin_pcode` is unpopulated in the current artifact (the
 * foundry hasn't joined postal codes to admin units directly), so the
 * admin chain on /v1/postal/:code is derived the same way /v1/reverse
 * derives it: cellId(lat, lon) → cell_lookup.gnd_pcode → parent chain. That
 * derivation is skipped (chain stays all-null) when the postal point falls
 * outside grid coverage or lands on a cell missing from cell_lookup —
 * neither is an error, just an incomplete but valid record.
 */
export function mountPostalRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/postal/:code", (c) => {
    const parsed = CodeQuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { lang } = parsed.data;

    const code = c.req.param("code");
    const row = findPostalByCode(db, code);
    if (!row) throw new NotFoundError(`postal code "${code}" not found`);

    const datasetIds = new Set<string>(["postal-codes"]);
    let chain: ReturnType<typeof resolveAdminLevels> = {
      gnd: null,
      ds_division: null,
      district: null,
      province: null,
    };
    if (row.lat != null && row.lon != null) {
      const id = cellId(row.lat, row.lon);
      if (id !== null) {
        const lookup = prepared<{ gnd_pcode: string }>(
          db,
          "SELECT gnd_pcode FROM cell_lookup WHERE cell_id = ?",
        ).get(id);
        if (lookup) {
          datasetIds.add("admin-units");
          chain = resolveAdminLevels(db, lookup.gnd_pcode, lang);
        }
      }
    }

    const payload = { code: row.code, name: row.name, lat: row.lat, lon: row.lon, ...chain };
    const meta = buildMeta(db, [...datasetIds]);
    return c.json(ok(payload, meta));
  });

  app.get("/v1/postal", (c) => {
    const parsed = PointQuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { lat, lon } = parsed.data;

    const id = cellId(lat, lon);
    if (id === null) throw new NotInCoverageError();

    const lookup = prepared<CellLookupPostalRow>(
      db,
      "SELECT postal_code FROM cell_lookup WHERE cell_id = ?",
    ).get(id);
    const postalCode = lookup?.postal_code ?? null;

    let name: string | null = null;
    if (postalCode) {
      const postal = findPostalByCode(db, postalCode);
      name = postal?.name ?? null;
    }

    const payload = { cell_id: id, postal_code: postalCode, name };
    const meta = buildMeta(db, ["postal-codes"]);
    return c.json(ok(payload, meta));
  });
}
