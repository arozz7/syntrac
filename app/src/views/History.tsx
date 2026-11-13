import { useState } from 'react';
import { SystemHistory } from './SystemHistory';

type TabId = 'system';

const TABS: Array<{ id: TabId; label: string }> = [{ id: 'system', label: 'System' }];

function CurrentTab({ tab }: { tab: TabId }) {
  switch (tab) {
    case 'system':
      return <SystemHistory />;
  }
}

/** Trends from the hourly summaries, which are kept long after the detailed data has been purged. */
export function History() {
  const [tab, setTab] = useState<TabId>('system');
  return (
    <>
      <div className="toolbar">
        <div className="range" role="group" aria-label="History view">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              className="range__button"
              aria-pressed={t.id === tab}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>
      <CurrentTab tab={tab} />
    </>
  );
}
