/**
 * Side-effect font imports for the Explore surface (Geist / Geist Mono —
 * see DESIGN-NOTES.md's "Design tokens" section). Imported once from
 * map-page.tsx. Both packages are already in web/package.json but weren't
 * imported anywhere yet; Vite/CSS treats repeated imports of the same file
 * as a no-op, so this stays safe even if another surface starts importing
 * the same weights later.
 */
import "@fontsource/geist/400.css";
import "@fontsource/geist/500.css";
import "@fontsource/geist/600.css";
import "@fontsource/geist/700.css";
import "@fontsource/geist-mono/400.css";
import "@fontsource/geist-mono/500.css";
import "@fontsource/geist-mono/600.css";
