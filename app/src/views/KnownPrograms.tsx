import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { KnownProductView, KnownProgramsView, LookalikeView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { SignatureBadge } from '../components/IdentityBadge';
import { DEFAULT_HISTORY_RANGE_SECS } from '../components/historyRanges';
import { SortHeader } from '../components/SortHeader';
import { formatDateTime } from '../format';
import { locationNote } from '../identityLabels';
import { usePolled } from '../hooks/usePolled';
import { useSort } from '../hooks/useSort';
import type { Accessors } from '../sorting';
import { ProgramDetail } from './ProgramDetail';

const REFRESH_MS = 60_000;

/** Remove, with a question first: it is one click to undo by marking again, but the list is the user's own record. */
function RemoveButton({ product, onChanged }: { product: KnownProductView; onChanged: () => void }) {
  const api = useApi();
  const [asking, setAsking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const remove = async () => {
    const outcome = await api.unmarkKnown(product.id);
    if (outcome.ok) {
      setAsking(false);
      onChanged();
    } else {
      setProblem(outcome.message ?? 'The change could not be saved.');
    }
  };

  if (!asking) {
    return (
      <button
        type="button"
        className="range__button"
        aria-label={`Remove ${product.label}`}
        onClick={() => setAsking(true)}
      >
        Remove
      </button>
    );
  }
  return (
    <div>
      <p>Remove {product.label} from the known programs?</p>
      {problem !== null && <p role="alert">{problem}</p>}
      <button type="button" className="range__button" onClick={() => void remove()}>
        Yes, remove
      </button>{' '}
      <button
        type="button"
        className="range__button"
        onClick={() => {
          setAsking(false);
          setProblem(null);
        }}
      >
        Keep
      </button>
    </div>
  );
}

const PRODUCT_SORTABLE: Accessors<KnownProductView> = {
  label: (product) => product.label,
  publisher: (product) => product.publisher ?? '',
  folder: (product) => product.folder,
  added: (product) => product.createdMs,
};

function ProductsTable({ products, onChanged }: { products: KnownProductView[]; onChanged: () => void }) {
  const sort = useSort();
  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="label" label="Label" />
          <SortHeader sort={sort} column="publisher" label="Signed by / company" />
          <SortHeader sort={sort} column="folder" label="Folder" />
          <th>Programs here</th>
          <th>Note</th>
          <SortHeader sort={sort} column="added" label="Added" firstDirection="desc" />
          <th />
        </tr>
      </thead>
      <tbody>
        {sort.apply(products, PRODUCT_SORTABLE).map((product) => (
          <tr key={product.id}>
            <td>{product.label}</td>
            <td>{product.publisher ?? 'none (unsigned, no company)'}</td>
            <td className="path">{product.folder}</td>
            <td>{product.programs.join(', ')}</td>
            <td>{product.note ?? '—'}</td>
            <td>{formatDateTime(product.createdMs)}</td>
            <td>
              <RemoveButton product={product} onChanged={onChanged} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const LOOKALIKE_SORTABLE: Accessors<LookalikeView> = {
  program: (program) => program.name,
  looksLike: (program) => program.standing.label,
};

function LookalikesTable({
  lookalikes,
  onSelect,
}: {
  lookalikes: LookalikeView[];
  onSelect: (choice: { name: string; path: string }) => void;
}) {
  const sort = useSort();
  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="program" label="Program" />
          <SortHeader sort={sort} column="looksLike" label="Looks like" />
          <th>Why</th>
          <th>Signature</th>
        </tr>
      </thead>
      <tbody>
        {sort.apply(lookalikes, LOOKALIKE_SORTABLE).map((program) => {
          const note = locationNote(program.location);
          return (
            <tr key={program.path}>
              <td>
                <button
                  type="button"
                  className="linklike"
                  onClick={() => onSelect({ name: program.name, path: program.path })}
                >
                  {program.name}
                </button>
                <div className="muted path">{program.path}</div>
              </td>
              <td>{program.standing.label}</td>
              <td>
                {program.standing.reasons.map((reason) => (
                  <div key={reason}>{reason}</div>
                ))}
              </td>
              <td>
                <div className="identity-cell">
                  <SignatureBadge
                    identity={{
                      state: 'known',
                      signature: program.signature,
                      publisher: program.publisher,
                      product: null,
                      location: program.location,
                      runsAs: [],
                      service: false,
                      startsAtBoot: false,
                      registeredAs: null,
                    }}
                  />
                  {note !== null && <span className="muted">{note}</span>}
                </div>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function Lists({
  view,
  onChanged,
  onSelect,
}: {
  view: KnownProgramsView;
  onChanged: () => void;
  onSelect: (choice: { name: string; path: string }) => void;
}) {
  return (
    <>
      <section className="card card--wide">
        <h2>Known programs</h2>
        {view.products.length === 0 ? (
          <p className="muted">
            No known programs yet. Open a program in the Programs tab and choose Mark as known to confirm it: the
            product (the same maker in the same folder) then shows as known, and anything that claims to be it but is
            not is flagged here.
          </p>
        ) : (
          <ProductsTable products={view.products} onChanged={onChanged} />
        )}
      </section>
      <section className="card card--wide">
        <h2>Programs that look like a known one</h2>
        {view.lookalikes.length === 0 ? (
          <p className="muted">Nothing looks like a known program.</p>
        ) : (
          <LookalikesTable lookalikes={view.lookalikes} onSelect={onSelect} />
        )}
        <p className="muted">
          A program is listed when it names a known product in its version information but is not from that maker or
          folder, or when it has the file name of a known program without a valid signature. That is a reason to look,
          not proof of anything.
        </p>
      </section>
    </>
  );
}

/** The programs the user has confirmed, and the programs that imitate them. */
export function KnownPrograms() {
  const api = useApi();
  const [version, setVersion] = useState(0);
  const [selected, setSelected] = useState<{ name: string; path: string } | null>(null);
  const result = usePolled(() => api.knownPrograms(), REFRESH_MS, `known:${version}`);
  const changed = () => setVersion((current) => current + 1);

  return (
    <>
      <DataGate result={result}>{(view) => <Lists view={view} onChanged={changed} onSelect={setSelected} />}</DataGate>
      {selected && (
        <ProgramDetail
          name={selected.name}
          path={selected.path}
          rangeSecs={DEFAULT_HISTORY_RANGE_SECS}
          onClose={() => setSelected(null)}
          onChanged={changed}
          version={version}
        />
      )}
    </>
  );
}
