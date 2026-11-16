# Phase S1: sortable tables

Slice S1 of `docs/ALERTS-PLAN.md` (approved 2026-11-16). Frontend only: no schema, service or pipe change.

## What changed
- `app/src/sorting.ts` (pure): `sortRows` (stable; numbers by value, text ignoring case with digits read as numbers; empty values
  (null, undefined, NaN) always last in both directions; a column without an accessor keeps the given order) and `nextSort`
  (first click in the column's first direction, second reverses, third clears back to the order the data came in).
- `app/src/hooks/useSort.ts`: the sort choice of one table, kept while its view is open (hoisted into the view, not the table,
  so a refresh that briefly replaces the table does not lose it).
- `app/src/components/SortHeader.tsx`: a `th` with a real button, `aria-sort` and a small arrow. Text columns start ascending;
  number and date columns start with the biggest or newest.
- Tables with sortable headings: Programs history, Network history (top destinations), New destinations, Processes, Connections,
  Overview (network destinations), destination detail (programs), Known programs (products, look-alikes). Identity, Why, Note and
  Signature columns are not sortable (they are badges or free text).
- `theme/app.css`: `.sortbutton` styles.
- Footnotes of Programs history and Network history say that sorting reorders the rows listed (the server returns the busiest N for
  the chosen ranking) and that a third click returns to that ranking.

## Tests (written first)
`sorting.test.ts` (10), `components/SortHeader.test.tsx` (4: unsorted start, ascending/descending/clear, number column first
direction and only the active column marked, keyboard), `NetworkHistory.test.tsx` (+1: a heading sorts the listed destinations and
the third click restores the given order). One existing test now finds the ranking button inside its own group, because a
"Programs" column heading is also a button.

## Limits (as designed)
Sorting works on the rows the server returned (a top-N list), not on the whole history; asking the server for a different top-N by
column is a possible later change. The choice is not kept after leaving the view.
