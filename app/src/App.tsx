import { useState } from 'react';
import { useApi } from './api/ApiContext';
import markUrl from './assets/brand/syntrac-mark.svg';
import { StatusBanner } from './components/StatusBanner';
import { usePolled } from './hooks/usePolled';
import { Connections } from './views/Connections';
import { Findings } from './views/Findings';
import { History } from './views/History';
import { Overview } from './views/Overview';
import { Processes } from './views/Processes';

type ViewId = 'overview' | 'processes' | 'connections' | 'findings' | 'history';

const VIEWS: Array<{ id: ViewId; nav: string; title: string }> = [
  { id: 'overview', nav: 'Overview', title: 'Workstation Overview' },
  { id: 'processes', nav: 'Processes', title: 'Processes' },
  { id: 'connections', nav: 'Connections', title: 'Connections' },
  { id: 'findings', nav: 'Findings', title: 'Findings' },
  { id: 'history', nav: 'History', title: 'History' },
];

const HEALTH_REFRESH_MS = 5000;
const FINDINGS_REFRESH_MS = 30_000;
const NOTIFICATIONS_REFRESH_MS = 5000;

function CurrentView({ view }: { view: ViewId }) {
  switch (view) {
    case 'overview':
      return <Overview />;
    case 'processes':
      return <Processes />;
    case 'connections':
      return <Connections />;
    case 'findings':
      return <Findings />;
    case 'history':
      return <History />;
  }
}

export function App() {
  const api = useApi();
  const [view, setView] = useState<ViewId>('overview');
  const polled = usePolled(
    async () => ({ health: await api.health(), fetchedAtMs: Date.now() }),
    HEALTH_REFRESH_MS,
    'health',
  );
  const health = polled?.health;
  // How many findings are open, for the badge beside Findings; nothing is shown while it cannot be read.
  const findings = usePolled(() => api.findings('open'), FINDINGS_REFRESH_MS, 'open-findings');
  const openFindings = findings?.state === 'ok' ? findings.data.counts.open : 0;
  // The program sends the toasts (with or without a window); this is only its switch, which the tray menu can change too.
  const [toastsOn, setToastsOn] = useState(true);
  const toastsSetting = usePolled(() => api.notificationsOn(), NOTIFICATIONS_REFRESH_MS, 'notifications');
  // What the program says wins whenever it changes (for example from the tray menu); a click shows its result at once.
  const [lastSetting, setLastSetting] = useState<boolean | undefined>(undefined);
  if (toastsSetting !== lastSetting) {
    setLastSetting(toastsSetting);
    if (toastsSetting !== undefined) {
      setToastsOn(toastsSetting);
    }
  }
  const current = VIEWS.find((v) => v.id === view) ?? VIEWS[0];

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <img src={markUrl} alt="" width={28} height={30} />
          <span>SYNTRAC</span>
        </div>
        <nav aria-label="Views">
          {VIEWS.map((v) => (
            <button
              key={v.id}
              type="button"
              className="nav"
              aria-current={v.id === view ? 'page' : undefined}
              onClick={() => setView(v.id)}
            >
              {v.nav}
              {v.id === 'findings' && openFindings > 0 && (
                <span className="nav__badge" role="img" aria-label={`${openFindings} open`}>
                  {openFindings}
                </span>
              )}
            </button>
          ))}
        </nav>
        <button
          type="button"
          className="nav nav--quiet"
          aria-pressed={toastsOn}
          onClick={() => {
            setToastsOn(!toastsOn);
            void api.setNotificationsOn(!toastsOn);
          }}
        >
          Toasts: {toastsOn ? 'on' : 'off'}
        </button>
        <p className="sidebar__foot muted">Local only. Nothing leaves this machine.</p>
      </aside>

      <main className="main">
        <header className="main__header">
          <h1>{current?.title}</h1>
          <StatusBanner health={health} nowMs={polled?.fetchedAtMs} />
        </header>
        <CurrentView view={view} />
      </main>
    </div>
  );
}
