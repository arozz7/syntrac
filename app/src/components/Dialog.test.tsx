// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Dialog } from './Dialog';

afterEach(cleanup);

function Opener({ onClose = () => undefined }: { onClose?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open it
      </button>
      {open && (
        <Dialog
          title="Details"
          onClose={() => {
            onClose();
            setOpen(false);
          }}
        >
          <p>Inside text</p>
          <button type="button">Inner action</button>
        </Dialog>
      )}
    </>
  );
}

describe('Dialog', () => {
  it('is a labelled modal dialog with its title as the heading', async () => {
    render(<Opener />);
    await userEvent.click(screen.getByRole('button', { name: 'Open it' }));

    const dialog = screen.getByRole('dialog', { name: 'Details' });
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(screen.getByRole('heading', { name: 'Details' })).toBeTruthy();
    expect(screen.getByText('Inside text')).toBeTruthy();
  });

  it('closes with the Close button, with Escape, and with a click outside, but not with a click inside', async () => {
    const onClose = vi.fn();
    render(<Opener onClose={onClose} />);

    await userEvent.click(screen.getByRole('button', { name: 'Open it' }));
    await userEvent.click(screen.getByText('Inside text'));
    expect(onClose).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('dialog')).toBeNull();

    await userEvent.click(screen.getByRole('button', { name: 'Open it' }));
    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledTimes(2);

    await userEvent.click(screen.getByRole('button', { name: 'Open it' }));
    await userEvent.click(screen.getByTestId('dialog-backdrop'));
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it('moves focus into the dialog and gives it back to what opened it', async () => {
    render(<Opener />);
    const opener = screen.getByRole('button', { name: 'Open it' });
    await userEvent.click(opener);

    expect(screen.getByRole('dialog').contains(document.activeElement)).toBe(true);

    await userEvent.keyboard('{Escape}');
    expect(document.activeElement).toBe(opener);
  });
});
