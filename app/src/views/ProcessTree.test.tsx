// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { ProcessNodeView, ProcessTreeView } from '../api/types';
import { makeFakeApi, processTreeView, treeNode } from '../test/fakeApi';
import { ProcessTree } from './ProcessTree';

// jsdom has no canvas; the program details draw charts.
vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <ProcessTree />
    </ApiProvider>,
  );
  return api;
}

/** An api whose tree is this one. */
function apiWith(view: ProcessTreeView) {
  return makeFakeApi({ processTree: vi.fn(async () => ({ state: 'ok' as const, data: view })) });
}

/** A chain of this many processes, each started by the one before. */
function chain(length: number): ProcessNodeView {
  let node = treeNode({
    id: `c${length}`,
    pid: length,
    name: `level${length}.exe`,
    imagePath: `C:\\chain\\l${length}.exe`,
  });
  for (let level = length - 1; level >= 1; level -= 1) {
    node = treeNode({
      id: `c${level}`,
      pid: level,
      name: `level${level}.exe`,
      imagePath: `C:\\chain\\l${level}.exe`,
      children: [node],
    });
  }
  return node;
}

describe('ProcessTree', () => {
  it('shows who started whom, a child nested under its starter', async () => {
    show();
    const explorer = (await screen.findByText('explorer.exe')).closest('li')!;
    const editor = within(explorer).getByText('editor.exe').closest('li')!;
    expect(within(editor).getByText('compiler.exe')).toBeTruthy();
  });

  it('says whether a process is still running or when it ended', async () => {
    show();
    const compiler = (await screen.findByText('compiler.exe')).closest('li')!;
    expect(within(compiler).getByText(/ended/i)).toBeTruthy();
    const editor = screen.getByText('editor.exe').closest('li')!;
    expect(within(editor).getAllByText(/running/i).length).toBeGreaterThan(0);
  });

  it('marks an approximate end time as approximate', async () => {
    const node = treeNode({ endedMs: 1_700_000_000_000, endedApproximate: true, running: false });
    show(apiWith(processTreeView({ roots: [node] })));
    expect(await screen.findByText(/about/i)).toBeTruthy();
  });

  it('marks a process whose starter is not in the window', async () => {
    show();
    const worker = (await screen.findByText('worker.exe')).closest('li')!;
    expect(within(worker).getByText('parent not recorded')).toBeTruthy();
    const explorer = screen.getByText('explorer.exe').closest('li')!;
    expect(within(explorer).queryByText('parent not recorded')).toBeNull();
  });

  it('shows the command line, which is text and never markup', async () => {
    const node = treeNode({ commandLine: 'tool.exe <b>bold</b> --flag' });
    show(apiWith(processTreeView({ roots: [node] })));
    expect(await screen.findByText('tool.exe <b>bold</b> --flag')).toBeTruthy();
  });

  it('collapses and expands a process with children', async () => {
    show();
    await screen.findByText('compiler.exe');
    await userEvent.click(screen.getByRole('button', { name: 'Collapse editor.exe' }));
    expect(screen.queryByText('compiler.exe')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Expand editor.exe' }));
    expect(screen.getByText('compiler.exe')).toBeTruthy();
  });

  it('starts with the deep levels collapsed, so a long chain does not fill the screen', async () => {
    const api = apiWith(processTreeView({ roots: [chain(6)] }));
    show(api);
    await screen.findByText('level1.exe');
    expect(screen.getByText('level2.exe')).toBeTruthy();
    expect(screen.queryByText('level4.exe')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Expand level3.exe' }));
    expect(screen.getByText('level4.exe')).toBeTruthy();
  });

  it('can expand and collapse everything at once', async () => {
    const api = apiWith(processTreeView({ roots: [chain(6)] }));
    show(api);
    await screen.findByText('level1.exe');
    await userEvent.click(screen.getByRole('button', { name: 'Expand all' }));
    expect(screen.getByText('level6.exe')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: 'Collapse all' }));
    expect(screen.queryByText('level2.exe')).toBeNull();
    expect(screen.getByText('level1.exe')).toBeTruthy();
  });

  it('says how many processes were cut from the tree', async () => {
    const node = treeNode({ hiddenDescendants: 7 });
    const data = processTreeView({ roots: [node], total: 9, shown: 2 });
    show(apiWith(data));
    expect(await screen.findByText('7 more not shown')).toBeTruthy();
    expect(screen.getByText(/2 of 9 processes/)).toBeTruthy();
  });

  it('opens the details of a program when its name is chosen', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'editor.exe' }));
    await waitFor(() =>
      expect(api.programHistory).toHaveBeenCalledWith(expect.any(Number), String.raw`C:\apps\editor.exe`),
    );
    expect(await screen.findByRole('heading', { name: 'editor.exe' })).toBeTruthy();
  });

  it('shows the identity badge of each program', async () => {
    show();
    const editor = (await screen.findByText('editor.exe')).closest('li')!;
    expect(within(editor).getAllByText(/signed/i).length).toBeGreaterThan(0);
  });

  it('reads a longer window when asked', async () => {
    const api = show();
    await screen.findByText('explorer.exe');
    expect(api.processTree).toHaveBeenLastCalledWith(24 * 3600);
    await userEvent.click(screen.getByRole('button', { name: '7 days' }));
    await waitFor(() => expect(api.processTree).toHaveBeenLastCalledWith(7 * 24 * 3600));
  });

  it('says so when no process was alive in the window', async () => {
    const data = processTreeView({ roots: [], total: 0, shown: 0 });
    show(apiWith(data));
    expect(await screen.findByText(/No processes were recorded/)).toBeTruthy();
  });

  it('explains the limits: detail is kept for 14 days and a parent is only known when it was seen to start', async () => {
    show();
    await screen.findByText('explorer.exe');
    expect(screen.getByText(/kept for 14 days/)).toBeTruthy();
  });
});
