import { useState } from 'react';
import { Dialog } from '../components/Dialog';
import { DestinationDetail } from './DestinationDetail';
import { ProgramDetailBody } from './ProgramDetail';

/** What the Network pop-up is showing: one destination, or one program reached from it (or from a row). */
export type NetworkFocus =
  { kind: 'destination'; addr: string } | { kind: 'program'; name: string; path: string; fromAddr: string | null };

/**
 * The pop-up of the Network tab. It stays one dialog while its content changes, so closing it gives focus back to where it was
 * opened from. A program opened from a destination has a "Back to <address>" button.
 */
export function NetworkDialog({
  focus,
  rangeSecs,
  onChange,
  onClose,
}: {
  focus: NetworkFocus;
  rangeSecs: number;
  onChange: (focus: NetworkFocus) => void;
  onClose: () => void;
}) {
  // Marking a program as known changes what its identity panel shows: read it again.
  const [version, setVersion] = useState(0);
  const fromAddr = focus.kind === 'program' ? focus.fromAddr : null;

  return (
    <Dialog
      title={focus.kind === 'destination' ? focus.addr : focus.name}
      onClose={onClose}
      actions={
        fromAddr === null ? undefined : (
          <button
            type="button"
            className="range__button"
            onClick={() => onChange({ kind: 'destination', addr: fromAddr })}
          >
            Back to {fromAddr}
          </button>
        )
      }
    >
      {focus.kind === 'destination' ? (
        <DestinationDetail
          addr={focus.addr}
          rangeSecs={rangeSecs}
          onOpenProgram={(program) => onChange({ kind: 'program', ...program, fromAddr: focus.addr })}
        />
      ) : (
        <ProgramDetailBody
          name={focus.name}
          path={focus.path}
          rangeSecs={rangeSecs}
          onChanged={() => setVersion((current) => current + 1)}
          version={version}
        />
      )}
    </Dialog>
  );
}
