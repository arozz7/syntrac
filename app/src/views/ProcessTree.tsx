import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ProcessNodeView, ProcessTreeView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { SignatureBadge } from '../components/IdentityBadge';
import { RangePicker, type RangeOption } from '../components/RangePicker';
import { DEFAULT_HISTORY_RANGE_SECS } from '../components/historyRanges';
import { formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { ProgramDetail } from './ProgramDetail';

const REFRESH_MS = 30_000;
const DAY_SECS = 86_400;
/** Levels shown open when the tree first appears; deeper ones are one click away. */
const OPEN_LEVELS = 2;

const WINDOWS: RangeOption[] = [
  { label: '1 hour', secs: 3600 },
  { label: '6 hours', secs: 6 * 3600 },
  { label: '24 hours', secs: DAY_SECS },
  { label: '7 days', secs: 7 * DAY_SECS },
  { label: '14 days', secs: 14 * DAY_SECS },
];
const DEFAULT_WINDOW_SECS = DAY_SECS;

type Program = { name: string; path: string };
type Base = 'default' | 'all' | 'none';

/** What the user has opened or closed, over a starting point that is the same for every process. */
interface Folding {
  base: Base;
  overrides: Record<string, boolean>;
}

function isOpen(folding: Folding, id: string, depth: number): boolean {
  const chosen = folding.overrides[id];
  if (chosen !== undefined) {
    return chosen;
  }
  return folding.base === 'all' || (folding.base === 'default' && depth < OPEN_LEVELS);
}

function lifeText(node: ProcessNodeView): string {
  if (node.running) {
    return `Running since ${formatDateTime(node.startedMs)}`;
  }
  const end = node.endedMs === null ? '' : formatDateTime(node.endedMs);
  return `${formatDateTime(node.startedMs)}, ended ${node.endedApproximate ? 'about ' : ''}${end}`;
}

function NodeRow({
  node,
  open,
  onToggle,
  onOpenProgram,
}: {
  node: ProcessNodeView;
  open: boolean;
  onToggle: () => void;
  onOpenProgram: (program: Program) => void;
}) {
  const hasChildren = node.children.length > 0;
  return (
    <div className="tree__row">
      {hasChildren ? (
        <button
          type="button"
          className="tree__toggle"
          aria-expanded={open}
          aria-label={`${open ? 'Collapse' : 'Expand'} ${node.name}`}
          onClick={onToggle}
        >
          {open ? '▾' : '▸'}
        </button>
      ) : (
        <span className="tree__toggle tree__toggle--none" aria-hidden="true" />
      )}
      {node.imagePath === '' ? (
        <span className="tree__name">{node.name}</span>
      ) : (
        <button
          type="button"
          className="linklike tree__name"
          title={node.imagePath}
          onClick={() => onOpenProgram({ name: node.name, path: node.imagePath })}
        >
          {node.name}
        </button>
      )}
      <span className="muted">pid {node.pid}</span>
      <span className="muted">{lifeText(node)}</span>
      <SignatureBadge identity={node.identity} />
      {node.parentMissing && (
        <span
          className="badge badge--quiet"
          title="What started this process was not running in this window, or was not recorded."
        >
          parent not recorded
        </span>
      )}
    </div>
  );
}

function Branch({
  node,
  depth,
  folding,
  onToggle,
  onOpenProgram,
}: {
  node: ProcessNodeView;
  depth: number;
  folding: Folding;
  onToggle: (id: string, open: boolean) => void;
  onOpenProgram: (program: Program) => void;
}) {
  const open = isOpen(folding, node.id, depth);
  return (
    <li className="tree__item">
      <NodeRow node={node} open={open} onToggle={() => onToggle(node.id, !open)} onOpenProgram={onOpenProgram} />
      {node.commandLine !== null && <div className="tree__command">{node.commandLine}</div>}
      {open && node.children.length > 0 && (
        <ul className="tree__children">
          {node.children.map((child) => (
            <Branch
              key={child.id}
              node={child}
              depth={depth + 1}
              folding={folding}
              onToggle={onToggle}
              onOpenProgram={onOpenProgram}
            />
          ))}
        </ul>
      )}
      {node.hiddenDescendants > 0 && <div className="muted tree__more">{node.hiddenDescendants} more not shown</div>}
    </li>
  );
}

function Forest({ view, onOpenProgram }: { view: ProcessTreeView; onOpenProgram: (program: Program) => void }) {
  const [folding, setFolding] = useState<Folding>({ base: 'default', overrides: {} });
  if (view.roots.length === 0) {
    return <p className="muted">No processes were recorded in this period.</p>;
  }
  const toggle = (id: string, open: boolean) =>
    setFolding((current) => ({ ...current, overrides: { ...current.overrides, [id]: open } }));
  return (
    <section className="card card--wide">
      <div className="card__head">
        <p className="muted">
          {view.shown === view.total
            ? `${view.total} ${view.total === 1 ? 'process' : 'processes'}`
            : `${view.shown} of ${view.total} processes`}
        </p>
        <div className="range">
          <button type="button" className="range__button" onClick={() => setFolding({ base: 'all', overrides: {} })}>
            Expand all
          </button>
          <button type="button" className="range__button" onClick={() => setFolding({ base: 'none', overrides: {} })}>
            Collapse all
          </button>
        </div>
      </div>
      <ul className="tree">
        {view.roots.map((root) => (
          <Branch
            key={root.id}
            node={root}
            depth={0}
            folding={folding}
            onToggle={toggle}
            onOpenProgram={onOpenProgram}
          />
        ))}
      </ul>
    </section>
  );
}

/** Who started whom among the processes alive in a window, from the per-process detail the service keeps. */
export function ProcessTree() {
  const api = useApi();
  const [windowSecs, setWindowSecs] = useState(DEFAULT_WINDOW_SECS);
  const [selected, setSelected] = useState<Program | null>(null);
  const [version, setVersion] = useState(0);
  const result = usePolled(() => api.processTree(windowSecs), REFRESH_MS, `tree:${windowSecs}`);

  return (
    <>
      <div className="toolbar">
        <RangePicker value={windowSecs} onChange={setWindowSecs} options={WINDOWS} />
      </div>
      <DataGate result={result}>{(view) => <Forest view={view} onOpenProgram={setSelected} />}</DataGate>
      {selected && (
        <ProgramDetail
          name={selected.name}
          path={selected.path}
          rangeSecs={DEFAULT_HISTORY_RANGE_SECS}
          onClose={() => setSelected(null)}
          onChanged={() => setVersion((current) => current + 1)}
          version={version}
        />
      )}
      <p className="muted">
        Per-process detail is kept for 14 days, so that is as far back as this can look. A process is placed under the
        one that started it when that was seen at the time; a process whose starter was not running in the window (or
        was not recorded) is listed on its own. Windows can reuse a process number, so each process is matched to its
        starter by when it started, not by number alone. Showing a command line is a convenience: it is what the program
        was started with, and it can contain anything.
      </p>
    </>
  );
}
