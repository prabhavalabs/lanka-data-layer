import { fileURLToPath } from "node:url";
import path from "node:path";

// foundry/src/lib/paths.ts -> foundry/
const FOUNDRY_ROOT = path.resolve(fileURLToPath(import.meta.url), "../../..");

export const RAW_DIR = path.join(FOUNDRY_ROOT, "data", "raw");
export const ARTIFACTS_DIR = path.join(FOUNDRY_ROOT, "data", "artifacts");
export const DB_PATH = path.join(ARTIFACTS_DIR, "lanka.sqlite");
export const MANIFEST_PATH = path.join(ARTIFACTS_DIR, "manifest.json");

export function rawPath(...segments: string[]): string {
  return path.join(RAW_DIR, ...segments);
}

export function artifactPath(...segments: string[]): string {
  return path.join(ARTIFACTS_DIR, ...segments);
}
