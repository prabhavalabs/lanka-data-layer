import ExcelJS from "exceljs";
import type { CellValue } from "./census.ts";

/**
 * Reads the first worksheet of an .xlsx file into a plain cell matrix:
 * one array per sheet row, index = 0-based column (so matrix[r][1] is
 * column B). Numbers stay numbers; rich-text cells (the census tables'
 * trilingual name cells) collapse to their plain-text runs joined; anything
 * else non-null is stringified. Only cells actually present in the file get
 * an entry — absent trailing cells read back as undefined, which every
 * consumer treats the same as null.
 */
export async function readSheetMatrix(filePath: string): Promise<CellValue[][]> {
  const workbook = new ExcelJS.Workbook();
  await workbook.xlsx.readFile(filePath);
  const sheet = workbook.worksheets[0];
  if (!sheet) throw new Error(`xlsx: ${filePath} has no worksheets`);

  const matrix: CellValue[][] = [];
  sheet.eachRow({ includeEmpty: true }, (row, rowNumber) => {
    const cells: CellValue[] = [];
    row.eachCell({ includeEmpty: false }, (cell, colNumber) => {
      cells[colNumber - 1] = toCellValue(cell.value);
    });
    matrix[rowNumber - 1] = cells;
  });
  // eachRow skips fully-empty rows even with includeEmpty, leaving holes.
  for (let i = 0; i < matrix.length; i++) matrix[i] ??= [];
  return matrix;
}

function toCellValue(v: ExcelJS.CellValue): CellValue {
  if (v === null || v === undefined) return null;
  if (typeof v === "number" || typeof v === "string") return v;
  if (typeof v === "object" && "richText" in v) return v.richText.map((r) => r.text).join("");
  if (typeof v === "object" && "result" in v) return toCellValue(v.result as ExcelJS.CellValue);
  if (v instanceof Date) return v.toISOString();
  return String(v);
}
