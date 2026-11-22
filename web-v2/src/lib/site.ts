// The production address, in one place. The REST API and the MCP server live on the
// same origin as the site: the API under /v2, MCP at /mcp, bulk files under /downloads.
export const SITE_HOST = 'lanka-data-layer.prabhavalabs.com'
export const SITE = `https://${SITE_HOST}`
export const API = `${SITE}/v2`
export const MCP = `${SITE}/mcp`
export const DOWNLOADS = `${SITE}/downloads`
