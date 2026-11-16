import { useEffect, useId, useRef, type ReactNode } from 'react';

/**
 * A pop-up over the page: a labelled modal dialog with its title as the heading. Escape, the Close button and a click outside
 * close it; a click inside does not. Focus moves into it when it opens and goes back to what had it when it closes, so the
 * keyboard never gets lost behind it. The body scrolls inside, so nothing is ever below the fold.
 */
export function Dialog({
  title,
  onClose,
  children,
  actions,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Extra controls next to Close (for example "Back"). */
  actions?: ReactNode;
}) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  // The key handler below is set up once; it always calls the newest onClose.
  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    panel.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        close.current();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('keydown', onKey);
      before?.focus();
    };
  }, []);

  return (
    <div className="dialog__backdrop" data-testid="dialog-backdrop" onClick={onClose}>
      <div
        ref={panel}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="dialog__head">
          <h2 id={titleId}>{title}</h2>
          <div className="dialog__actions">
            {actions}
            <button type="button" className="range__button" onClick={onClose}>
              Close
            </button>
          </div>
        </div>
        <div className="dialog__body">{children}</div>
      </div>
    </div>
  );
}
