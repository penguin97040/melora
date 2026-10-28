import { act, render, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { type Page, type Paged, useEndOfList, usePaged } from './library';

type Fetch = (key: string, offset: number) => Promise<Page<number>>;

function page(offset: number, total = 200): Page<number> {
  const items = Array.from({ length: 50 }, (_, i) => offset + i);
  return {
    items,
    total,
    nextOffset: offset + 50 < total ? offset + 50 : null,
  };
}

const realObserver = globalThis.IntersectionObserver;
afterEach(() => {
  globalThis.IntersectionObserver = realObserver;
});

/** An observer that reports its element as visible straight away, like a footer on screen. */
function footerVisible() {
  globalThis.IntersectionObserver = class {
    constructor(private callback: IntersectionObserverCallback) {}
    observe() {
      this.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        this as unknown as IntersectionObserver,
      );
    }
    disconnect() {}
    unobserve() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
}

describe('usePaged', () => {
  it('asks once for a failed page while its footer stays on screen', async () => {
    footerVisible();
    const fetchPage = vi.fn<Fetch>((_key, offset) =>
      offset === 0 ? Promise.resolve(page(0)) : Promise.reject(new Error('x')),
    );
    let renders = 0;
    let list: Paged<number> | null = null;
    function List() {
      renders += 1;
      list = usePaged('liked', fetchPage);
      const end = useEndOfList(list);
      return <div ref={end} />;
    }
    render(<List />);
    await waitFor(() => expect(list?.error).toBeTruthy());
    const settled = renders;
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(fetchPage.mock.calls.map(([, offset]) => offset)).toEqual([0, 50]);
    expect(renders - settled).toBeLessThanOrEqual(1);

    // Try again asks once more; scrolling still doesn't.
    act(() => list?.retry());
    await waitFor(() => expect(fetchPage).toHaveBeenCalledTimes(3));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(fetchPage).toHaveBeenCalledTimes(3);
  });

  it('loads nothing more from the old rows while a reload is on its way', async () => {
    let reloaded: (page: Page<number>) => void = () => undefined;
    let calls = 0;
    const fetchPage = vi.fn<Fetch>((_key, offset) => {
      calls += 1;
      return calls === 1
        ? Promise.resolve(page(offset))
        : new Promise<Page<number>>((resolve) => {
            reloaded = resolve;
          });
    });
    const { result } = renderHook(() => usePaged('playlist:a', fetchPage));
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.reload());
    expect(result.current.loading).toBe(true);
    act(() => result.current.loadMore());
    expect(fetchPage.mock.calls.map(([, offset]) => offset)).toEqual([0, 0]);

    act(() => reloaded(page(0, 120)));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.total).toBe(120);
  });

  it('never shows one account’s rows for another under the same key', async () => {
    const fetchPage = vi.fn<Fetch>(() => Promise.resolve(page(0)));
    const { result, rerender } = renderHook(
      ({ account }) => usePaged('playlist:a', fetchPage, account),
      { initialProps: { account: 'kiri' } },
    );
    await waitFor(() => expect(result.current.items).toHaveLength(50));

    rerender({ account: 'aroha' });
    expect(result.current.items).toEqual([]);
    expect(result.current.loading).toBe(true);
    // The account isn't part of what Rust is asked for.
    expect(fetchPage.mock.calls.map(([key]) => key)).toEqual([
      'playlist:a',
      'playlist:a',
    ]);
  });
});
