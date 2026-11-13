import { useState } from 'react';
import { useApi } from './api/ApiContext';
import markUrl from './assets/brand/syntrac-mark.svg';
import { StatusBanner } from './components/StatusBanner';
import { usePolled } from './hooks/usePolled';
import { Connections } from './views/Connections';
import { Overview } from './views/Overview';
import { Processes } from './views/Processes';

type ViewId = 'overview' | 'processes' | 'connections';

const VIEWS: Array<{ id: ViewId; nav: string; title: string }> = [
  { id: 'overview', nav: 'Overview', title: 'Workstation Overview' },
  { id: 'processes', nav: 'Processes', title: 'Processes' },
  { id: 'connections', nav: 'Connections', title: 'Connections' },
];

const HEALTH_REFRESH_MS = 5000;

function CurrentView({ view }: { view: ViewId }) {
  switch (view) {
    case 'overview':
      return <Overview />;
    case 'processes':
      return <Processes />;
    case 'connections':
      return <Connections />;
  }
}

export function App() {
  const api = useApi();
  const [view, setView] = useState<ViewId>('overview');
  const health = usePolled(() => api.health(), HEALTH_REFRESH_MS, 'health');
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
            </button>
          ))}
        </nav>
        <p className="sidebar__foot muted">Local only. Nothing leaves this machine.</p>
      </aside>

      <main className="main">
        <header className="main__header">
          <h1>{current?.title}</h1>
          <StatusBanner health={health} />
        </header>
        <CurrentView view={view} />
      </main>
    </div>
  );
}
