import logoUrl from './assets/brand/syntrac-horizontal.svg';

const SEVERITY_TIERS = [
  {
    tier: 'unusual',
    label: 'Unusual',
    shape: '●',
    note: 'Differs from your baseline. A lead to review, not a verdict.',
  },
  { tier: 'suspicious', label: 'Suspicious', shape: '▲', note: 'Matches a detection rule.' },
  { tier: 'confirmed', label: 'Confirmed', shape: '⬢', note: 'Matches threat intelligence.' },
] as const;

export function App() {
  return (
    <main className="shell">
      <header className="shell__header">
        <h1>
          <img className="shell__logo" src={logoUrl} alt="SYNTRAC: See Everything. Understand Anything." />
        </h1>
      </header>

      <section className="card" aria-labelledby="tiers-heading">
        <h2 id="tiers-heading">Detection tiers</h2>
        <p className="muted">An anomaly is not a threat. Each tier has its own colour, label and shape.</p>
        <ul className="tiers">
          {SEVERITY_TIERS.map(({ tier, label, shape, note }) => (
            <li key={tier}>
              <span className={`chip chip--${tier}`}>
                <span aria-hidden="true">{shape}</span> {label}
              </span>
              <span className="muted">{note}</span>
            </li>
          ))}
        </ul>
      </section>

      <p className="muted">Phase 0 foundation shell. The dashboard arrives in Phase 1.</p>
    </main>
  );
}
