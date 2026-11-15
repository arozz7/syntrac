# Phase T3-0: cards keep their content inside (layout fix)

Slice T3-0 of `docs/NETWORK-PLAN.md`. The user reported that on some screens text spills past the edge of a card. No screenshot of
the clipped cards was available, so the fix follows the likely causes found in the code, and is NOT yet confirmed on screen.

## Causes found
- `.card` had `min-width: 0` but no overflow handling. A long unbroken value inside a card (a file path in the program detail, a
  certificate thumbprint, a SHA-256 in the identity panel, an IPv6 address, the "Runs from" folder) could not wrap and spilled out.
- A table wider than its card (many columns in a narrow window) had nowhere to go.
- Table numbers wrapped between value and unit ("633.7" / "KB"), which made rows uneven.

## What changed (`app/src/theme/app.css` only)
- `.card { overflow-wrap: anywhere; overflow-x: auto; }`: long text wraps at any character; a table that is still too wide scrolls
  inside its card instead of spilling past it.
- `.table .num { white-space: nowrap; }`: a number stays with its unit and the column grows.

## Tests (written first)
`app/src/theme/layout.test.ts` (3): reads `app.css` as text (jsdom has no layout) and asserts the three rules above exist.

## Not verified
Not looked at on screen. Please check the screens that clipped before (the program detail with its identity panel is the likeliest)
and send a screenshot of anything still wrong.
