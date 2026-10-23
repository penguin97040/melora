import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useWindow, visibleRange, WINDOW_FROM } from './library';

describe('visibleRange', () => {
  it('covers the rows on screen plus a margin, within the list', () => {
    // A 400 px window over 40 px rows, the list starting at the top: rows 0–9, plus 20 after.
    expect(visibleRange(0, 400, 40, 1000)).toEqual([0, 30]);
    // Scrolled 4,000 px down: rows 100–109 on screen.
    expect(visibleRange(-4000, 400, 40, 1000)).toEqual([80, 130]);
    // Near the end, and past it.
    expect(visibleRange(-39_800, 400, 40, 1000)).toEqual([975, 1000]);
    expect(visibleRange(-80_000, 400, 40, 1000)).toEqual([1000, 1000]);
    // Below the screen: nothing but the margin.
    expect(visibleRange(2000, 400, 40, 1000)).toEqual([0, 0]);
  });

  it('renders everything until rows can be measured', () => {
    expect(visibleRange(0, 400, 0, 500)).toEqual([0, 500]);
  });
});

function Rows({ count }: { count: number }) {
  const [body, rows] = useWindow(count);
  return (
    <table>
      <tbody ref={body}>
        {rows.start > 0 && (
          <tr className="spacer-row" data-testid="before">
            <td style={{ height: rows.start * rows.rowHeight }} />
          </tr>
        )}
        {Array.from({ length: rows.end - rows.start }, (_, i) => (
          <tr key={rows.start + i}>
            <td>Row {rows.start + i}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

describe('useWindow', () => {
  afterEach(() => vi.restoreAllMocks());

  it('renders short lists in full', () => {
    render(<Rows count={WINDOW_FROM - 1} />);
    expect(screen.getAllByRole('row')).toHaveLength(WINDOW_FROM - 1);
  });

  it('renders only the rows near the screen in long lists', async () => {
    vi.spyOn(window, 'innerHeight', 'get').mockReturnValue(400);
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(
      function (this: Element) {
        const height = this.tagName === 'TR' ? 40 : 40_000;
        return { top: -4000, height } as DOMRect;
      },
    );
    render(<Rows count={1000} />);
    await waitFor(() => expect(screen.getAllByRole('row')).toHaveLength(51));
    expect(screen.getByText('Row 80')).toBeInTheDocument();
    expect(screen.queryByText('Row 79')).not.toBeInTheDocument();
    expect(screen.queryByText('Row 130')).not.toBeInTheDocument();
    expect(screen.getByTestId('before').firstElementChild).toHaveStyle({
      height: '3200px',
    });
  });
});
