/// <reference types="vite/client" />
interface ImportMetaEnv {
  /** origin of the Lanka Data Layer API; empty = same origin (the dev server proxies /v2, /app and /dl) */
  readonly VITE_API_BASE?: string
}
interface ImportMeta {
  readonly env: ImportMetaEnv
}
