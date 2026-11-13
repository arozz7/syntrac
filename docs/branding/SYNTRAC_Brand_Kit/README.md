# SYNTRAC Brand Kit

This is the first editable source package for the SYNTRAC visual identity.

## Contents
- `BRAND_GUIDELINES.md` — brand direction, palette, typography, usage and UI principles.
- `assets/logos/` — SVG logo variations.
- `assets/icons/` — application and tray icon source SVGs.
- `styles/design-tokens.css` — reusable CSS color and component tokens.
- `mockups/dashboard-concept.html` — responsive, browser-viewable dashboard concept.

## Preview
Open `mockups/dashboard-concept.html` in a modern browser. It references the SVG logo and CSS tokens using relative paths, so keep the folder structure intact.

## Notes
The dashboard is a visual concept with illustrative sample telemetry, not a functioning monitoring interface. The logo is an initial vector direction and can be refined before production packaging. Convert the app icon SVG to the Windows `.ico` sizes required by the eventual Tauri build pipeline.
