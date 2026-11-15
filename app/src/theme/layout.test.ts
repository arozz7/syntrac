import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

/** The declarations of the first rule whose selector is exactly `selector`. jsdom has no layout, so the rules are read as text. */
function declarations(selector: string): string {
  const css = readFileSync(new URL('./app.css', import.meta.url), 'utf8');
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = new RegExp(`(?:^|\\})\\s*${escaped}\\s*\\{([^}]*)\\}`, 'm').exec(css);
  if (!match?.[1]) {
    throw new Error(`No rule for ${selector} in app.css`);
  }
  return match[1];
}

describe('cards keep their content inside', () => {
  it('lets long unbroken text (paths, hashes, IPv6 addresses) wrap instead of spilling past the card', () => {
    expect(declarations('.card')).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it('scrolls a table that is still too wide inside the card instead of spilling past it', () => {
    expect(declarations('.card')).toMatch(/overflow-x:\s*auto/);
  });

  it('keeps a number and its unit together, so a column grows rather than breaking "633.7 KB" over two lines', () => {
    expect(declarations('.table .num')).toMatch(/white-space:\s*nowrap/);
  });
});
