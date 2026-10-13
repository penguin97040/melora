import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { WindowHeader } from './WindowHeader';
import {
  closeWindow,
  doubleClickWindowHeader,
  isWindowMaximised,
  minimiseWindow,
  onWindowResize,
  startWindowDragging,
  startWindowResize,
  toggleWindowMaximise,
} from '../native/client';

vi.mock('../native/client', () => ({
  closeWindow: vi.fn(),
  doubleClickWindowHeader: vi.fn(),
  isWindowMaximised: vi.fn(),
  minimiseWindow: vi.fn(),
  onWindowResize: vi.fn(),
  startWindowDragging: vi.fn(),
  startWindowResize: vi.fn(),
  toggleWindowMaximise: vi.fn(),
}));
let resize: () => void;
const stop = vi.fn();
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(isWindowMaximised).mockResolvedValue(false);
  vi.mocked(onWindowResize).mockImplementation(async (handler) => {
    resize = handler;
    return stop;
  });
});

describe('integrated window header', () => {
  it('supports keyboard controls, native maximise changes, resize edges and listener cleanup', async () => {
    const user = userEvent.setup();
    const { container, unmount } = render(
      <WindowHeader chrome="custom" onError={vi.fn()} />,
    );
    await waitFor(() => expect(isWindowMaximised).toHaveBeenCalled());
    const minimise = screen.getByRole('button', { name: 'Minimise window' });
    minimise.focus();
    await user.keyboard('{Enter}');
    expect(minimiseWindow).toHaveBeenCalledTimes(1);
    await user.tab();
    await user.keyboard(' ');
    expect(toggleWindowMaximise).toHaveBeenCalledTimes(1);
    vi.mocked(isWindowMaximised).mockResolvedValue(true);
    await act(async () => resize());
    expect(
      screen.getByRole('button', { name: 'Restore window' }),
    ).toHaveFocus();
    expect(container.querySelectorAll('.resize-target')).toHaveLength(0);
    vi.mocked(isWindowMaximised).mockResolvedValue(false);
    await act(async () => resize());
    const targets = container.querySelectorAll('.resize-target');
    expect(targets).toHaveLength(8);
    for (const target of targets) {
      fireEvent.mouseDown(target, { button: 0 });
      await waitFor(() =>
        expect(startWindowResize).toHaveBeenCalledWith(
          target.getAttribute('data-resize-edge'),
        ),
      );
    }
    await user.click(screen.getByRole('button', { name: 'Close window' }));
    expect(closeWindow).toHaveBeenCalledTimes(1);
    unmount();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it('drags branding, handles double clicks and excludes control descendants', async () => {
    const { container } = render(
      <WindowHeader chrome="custom" onError={vi.fn()} />,
    );
    fireEvent.mouseDown(screen.getByText('melora'), { button: 0, detail: 1 });
    await waitFor(() => expect(startWindowDragging).toHaveBeenCalledTimes(1));
    fireEvent.mouseDown(container.querySelector('.brand-symbol')!, {
      button: 0,
      detail: 2,
    });
    await waitFor(() =>
      expect(doubleClickWindowHeader).toHaveBeenCalledTimes(1),
    );
    fireEvent.mouseDown(container.querySelector('.window-controls svg path')!, {
      button: 0,
      detail: 1,
    });
    fireEvent.mouseDown(screen.getByText('melora'), { button: 2, detail: 1 });
    expect(startWindowDragging).toHaveBeenCalledTimes(1);
  });

  it('preserves macOS mouse-up double-click behaviour and cancels moved clicks', async () => {
    render(<WindowHeader chrome="macos" onError={vi.fn()} />);
    const brand = screen.getByText('melora');
    expect(
      screen.queryByRole('group', { name: 'Window controls' }),
    ).not.toBeInTheDocument();
    fireEvent.mouseDown(brand, {
      button: 0,
      detail: 2,
      clientX: 100,
      clientY: 20,
    });
    expect(doubleClickWindowHeader).not.toHaveBeenCalled();
    fireEvent.mouseUp(brand, {
      button: 0,
      detail: 2,
      clientX: 101,
      clientY: 20,
    });
    expect(doubleClickWindowHeader).not.toHaveBeenCalled();
    fireEvent.mouseDown(brand, {
      button: 0,
      detail: 2,
      clientX: 100,
      clientY: 20,
    });
    fireEvent.mouseUp(brand, {
      button: 0,
      detail: 2,
      clientX: 100,
      clientY: 20,
    });
    await waitFor(() =>
      expect(doubleClickWindowHeader).toHaveBeenCalledTimes(1),
    );
  });

  it('disables browser controls without native subscriptions or dragging', () => {
    const { container } = render(
      <WindowHeader chrome="preview" onError={vi.fn()} />,
    );
    for (const button of screen.getAllByRole('button'))
      expect(button).toBeDisabled();
    expect(onWindowResize).not.toHaveBeenCalled();
    expect(container.querySelector('.resize-target')).toBeNull();
    fireEvent.mouseDown(screen.getByText('melora'), { button: 0, detail: 1 });
    expect(startWindowDragging).not.toHaveBeenCalled();
  });

  it('cleans up a subscription that resolves after unmount', async () => {
    let resolve!: (stop: () => void) => void;
    vi.mocked(onWindowResize).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const { unmount } = render(
      <WindowHeader chrome="custom" onError={vi.fn()} />,
    );
    unmount();
    await act(async () => resolve(stop));
    expect(stop).toHaveBeenCalledTimes(1);
    expect(isWindowMaximised).not.toHaveBeenCalled();
  });

  it('ignores stale window queries and reports native operation failures safely', async () => {
    const onError = vi.fn();
    const { container } = render(
      <WindowHeader chrome="custom" onError={onError} />,
    );
    await waitFor(() => expect(isWindowMaximised).toHaveBeenCalled());
    let resolveOld!: (value: boolean) => void;
    vi.mocked(isWindowMaximised).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
    await act(async () => resize());
    vi.mocked(isWindowMaximised).mockResolvedValue(true);
    await act(async () => resize());
    await act(async () => resolveOld(false));
    expect(
      screen.getByRole('button', { name: 'Restore window' }),
    ).toBeInTheDocument();
    vi.mocked(startWindowDragging).mockRejectedValue(
      new Error('synthetic internal failure'),
    );
    fireEvent.mouseDown(container.querySelector('.masthead')!, {
      button: 0,
      detail: 1,
    });
    await waitFor(() =>
      expect(onError).toHaveBeenCalledWith(
        'The window operation failed. Try again.',
      ),
    );
  });
  it('reads the initial maximise state even when resize subscription fails', async () => {
    vi.mocked(onWindowResize).mockRejectedValue(
      new Error('synthetic subscription failure'),
    );
    vi.mocked(isWindowMaximised).mockResolvedValue(true);
    const onError = vi.fn();
    const { container } = render(
      <WindowHeader chrome="custom" onError={onError} />,
    );
    await screen.findByRole('button', { name: 'Restore window' });
    expect(container.querySelectorAll('.resize-target')).toHaveLength(0);
    expect(onError).toHaveBeenCalledWith(
      'Window changes could not be monitored.',
    );
  });

  it('ignores failures from obsolete maximise queries', async () => {
    const onError = vi.fn();
    render(<WindowHeader chrome="custom" onError={onError} />);
    await waitFor(() => expect(isWindowMaximised).toHaveBeenCalled());
    let rejectOld!: (error: Error) => void;
    vi.mocked(isWindowMaximised).mockReturnValueOnce(
      new Promise((_resolve, reject) => {
        rejectOld = reject;
      }),
    );
    await act(async () => resize());
    vi.mocked(isWindowMaximised).mockResolvedValue(true);
    await act(async () => resize());
    await act(async () => rejectOld(new Error('synthetic stale failure')));
    expect(onError).not.toHaveBeenCalled();
    expect(
      screen.getByRole('button', { name: 'Restore window' }),
    ).toBeInTheDocument();
  });
});
