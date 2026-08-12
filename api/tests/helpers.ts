/**
 * @types/node's undici-based Response#json() types as Promise<unknown> (by
 * design, to force callers to validate). Tests just want the envelope back
 * as a plain object to poke at, so this is the one sanctioned `any` escape
 * hatch instead of casting at every call site.
 */
export async function readJson(res: Response): Promise<any> {
  return res.json();
}
