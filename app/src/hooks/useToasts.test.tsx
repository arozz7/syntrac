// @vitest-environment jsdom
import { renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { FindingView } from '../api/types';
import { findingRow } from '../test/fakeApi';
import { useToasts } from './useToasts';

const suspicious = (id: number): FindingView =>
  findingRow({ id, tier: 'suspicious', title: `Finding ${id}`, why: 'Why.' });

describe('useToasts', () => {
  it('shows nothing for the first list, then a toast for a new suspicious finding', () => {
    const notify = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ list }) => useToasts(list, true, notify), {
      initialProps: { list: [suspicious(1)] as FindingView[] | undefined },
    });
    expect(notify).not.toHaveBeenCalled();

    rerender({ list: [suspicious(1), suspicious(2)] });

    expect(notify).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith('SYNTRAC: suspicious finding', 'Finding 2. Why.');
  });

  it('says each finding only once, however often the list is read again', () => {
    const notify = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ list }) => useToasts(list, true, notify), {
      initialProps: { list: [] as FindingView[] | undefined },
    });

    rerender({ list: [suspicious(5)] });
    rerender({ list: [suspicious(5)] });
    rerender({ list: [suspicious(5)] });

    expect(notify).toHaveBeenCalledTimes(1);
  });

  it('shows nothing while the list cannot be read, and nothing when toasts are switched off', () => {
    const notify = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ list, on }) => useToasts(list, on, notify), {
      initialProps: { list: undefined as FindingView[] | undefined, on: true },
    });
    rerender({ list: undefined, on: true });
    expect(notify).not.toHaveBeenCalled();

    rerender({ list: [], on: false });
    rerender({ list: [suspicious(9)], on: false });
    expect(notify).not.toHaveBeenCalled();
  });

  it('a finding that appeared while toasts were off is not announced when they are switched on', () => {
    const notify = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ list, on }) => useToasts(list, on, notify), {
      initialProps: { list: [] as FindingView[] | undefined, on: false },
    });

    rerender({ list: [suspicious(1)], on: false });
    rerender({ list: [suspicious(1)], on: true });

    expect(notify).not.toHaveBeenCalled();
  });
});
