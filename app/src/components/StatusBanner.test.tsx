// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { health } from '../test/fakeApi';
import { StatusBanner } from './StatusBanner';

afterEach(cleanup);

describe('StatusBanner', () => {
  it('says the collector is being checked before the first answer', () => {
    render(<StatusBanner health={undefined} />);
    expect(screen.getByRole('status').textContent).toContain('Checking');
  });

  it.each([
    ['running', 'Running'],
    ['stale', 'Stale'],
    ['degraded', 'Degraded'],
    ['offline', 'Offline'],
    ['unknown', 'Unknown'],
  ] as const)('labels the %s state in words, never by colour alone', (state, label) => {
    render(<StatusBanner health={health({ state, message: `msg for ${state}` })} />);
    const banner = screen.getByRole('status');
    expect(banner.getAttribute('data-state')).toBe(state);
    expect(banner.textContent).toContain(label);
    expect(banner.textContent).toContain(`msg for ${state}`);
  });

  it('shows how fresh the data is and how long the collector has been up', () => {
    render(<StatusBanner health={health({ dataAgeSecs: 12, uptimeSecs: 3661 })} />);
    const text = screen.getByRole('status').textContent ?? '';
    expect(text).toContain('12 s ago');
    expect(text).toContain('1 h 1 min');
  });

  it('calls out dropped ETW events when there are any, and says nothing when there are none', () => {
    const { rerender } = render(<StatusBanner health={health({ etwDropped: 7 })} />);
    expect(screen.getByRole('status').textContent).toContain('7 events dropped');
    rerender(<StatusBanner health={health({ etwDropped: 0 })} />);
    expect(screen.getByRole('status').textContent).not.toContain('dropped');
  });

  it('copes with an offline collector that has never written anything', () => {
    render(
      <StatusBanner
        health={health({ state: 'offline', pid: null, uptimeSecs: null, dataAgeSecs: null, etwDropped: null })}
      />,
    );
    expect(screen.getByRole('status').textContent).toContain('Offline');
  });
});

describe('StatusBanner: a silent ETW trace', () => {
  it('says how long ETW has been silent, so the reader knows what is being lost and for how long', () => {
    render(<StatusBanner health={health({ state: 'degraded', message: 'ETW is silent', etwSilentSecs: 4000 })} />);
    expect(screen.getByRole('status').textContent).toContain('ETW silent 1 h 6 min');
  });

  it('says nothing about ETW silence while it is only a few seconds', () => {
    render(<StatusBanner health={health({ etwSilentSecs: 8 })} />);
    expect(screen.getByRole('status').textContent).not.toContain('silent');
  });
});

describe('StatusBanner: storage', () => {
  const GB = 1024 ** 3;
  const NOW_MS = 1_800_000_000_000;
  const storage = {
    level: 'ok' as const,
    databaseLiveBytes: 3 * GB,
    databaseFileBytes: 3.2 * GB,
    driveFreeBytes: 120 * GB,
    oldestDetailMs: NOW_MS - 13 * 86_400_000,
    oldestRollupMs: NOW_MS - 40 * 86_400_000,
    trimmedHoursTotal: 0,
  };

  it('shows how big the database is, how much room the drive has, and how far back the detail and summaries go', () => {
    render(<StatusBanner health={health({ storage })} nowMs={NOW_MS} />);
    const text = screen.getByRole('status').textContent ?? '';
    expect(text).toContain('Database 3.0 GB');
    expect(text).toContain('Drive free 120.0 GB');
    expect(text).toContain('Detail back 13 d ago');
    expect(text).toContain('Summaries back 40 d ago');
  });

  it('says nothing about storage when the collector does not report it', () => {
    render(<StatusBanner health={health({ storage: null })} nowMs={NOW_MS} />);
    const text = screen.getByRole('status').textContent ?? '';
    expect(text).not.toContain('Database');
    expect(text).not.toContain('Drive free');
  });

  it('leaves out the ages that are not known yet (nothing is rolled up on a new database)', () => {
    render(
      <StatusBanner
        health={health({ storage: { ...storage, oldestDetailMs: null, oldestRollupMs: null } })}
        nowMs={NOW_MS}
      />,
    );
    const text = screen.getByRole('status').textContent ?? '';
    expect(text).toContain('Database');
    expect(text).not.toContain('Detail back');
    expect(text).not.toContain('Summaries back');
  });

  it('says how much has been trimmed early while the guard is trimming', () => {
    render(
      <StatusBanner
        health={health({
          state: 'degraded',
          message: 'getting full',
          storage: { ...storage, level: 'trimming', trimmedHoursTotal: 9 },
        })}
        nowMs={NOW_MS}
      />,
    );
    expect(screen.getByRole('status').textContent).toContain('9 h trimmed early');
  });

  it('does not mention trimming when none has happened', () => {
    render(<StatusBanner health={health({ storage })} nowMs={NOW_MS} />);
    expect(screen.getByRole('status').textContent).not.toContain('trimmed');
  });
});
