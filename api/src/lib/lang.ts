import type { Lang } from "@lanka-data-layer/shared";

export const LANGS: readonly Lang[] = ["en", "si", "ta"];

export function isLang(value: string): value is Lang {
  return (LANGS as readonly string[]).includes(value);
}

interface Trilingual {
  name_en: string;
  name_si?: string | null;
  name_ta?: string | null;
}

/** Resolves a trilingual name row to the requested language, falling back to English (contract §4). */
export function resolveName(row: Trilingual, lang: Lang): string {
  if (lang === "en") return row.name_en;
  const translated = lang === "si" ? row.name_si : row.name_ta;
  return translated && translated.trim() !== "" ? translated : row.name_en;
}
