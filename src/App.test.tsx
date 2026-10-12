import { act, render, screen, waitFor, within } from '@testing-library/react';
import { StrictMode } from 'react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import App from './App';
import { getAppInfo, savePreferences, showMainWindow } from './native/client';
import type { AppInfo } from './native/client';

vi.mock('./native/client', () => ({
  defaultPreferences: { scale: 100, comfortable: false, volume: 70 },
  getAppInfo: vi.fn(),
  savePreferences: vi.fn(),
  showMainWindow: vi.fn(),
  hasNativeStorage: () => true,
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.1.0',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
});

describe('preview shell', () => {
  it('reveals the committed shell once after restored preferences settle in Strict Mode', async () => {
    let resolveInfo!: (info: AppInfo) => void;
    vi.mocked(getAppInfo).mockReturnValue(
      new Promise((resolve) => {
        resolveInfo = resolve;
      }),
    );
    vi.mocked(showMainWindow).mockImplementation(async () => {
      expect(document.documentElement.style.fontSize).toBe('150%');
      expect(screen.getByText('v0.1.0 / windows')).toBeInTheDocument();
      expect(document.querySelector('.app')).toHaveClass('comfortable');
    });
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    expect(showMainWindow).not.toHaveBeenCalled();
    await act(async () =>
      resolveInfo({
        version: '0.1.0',
        platform: 'windows',
        storageError: null,
        preferences: { scale: 150, comfortable: true, volume: 40 },
      }),
    );
    expect(showMainWindow).toHaveBeenCalledTimes(1);
    await userEvent
      .setup()
      .click(screen.getByRole('button', { name: /Settings/ }));
    expect(showMainWindow).toHaveBeenCalledTimes(1);
  });

  it('reveals the default shell and an error when initial preferences fail', async () => {
    vi.mocked(getAppInfo).mockRejectedValue(new Error('fixture failure'));
    vi.mocked(showMainWindow).mockImplementation(async () => {
      expect(screen.getByRole('alert')).toHaveTextContent(
        'Native application details could not be loaded',
      );
      expect(document.documentElement.style.fontSize).toBe('100%');
    });
    render(<App />);
    await waitFor(() => expect(showMainWindow).toHaveBeenCalledTimes(1));
    await userEvent
      .setup()
      .click(screen.getByRole('button', { name: /Settings/ }));
    expect(screen.getByRole('combobox')).toBeDisabled();
  });

  it('does not reveal from a stale preference request after unmount', async () => {
    let rejectInfo!: (error: Error) => void;
    vi.mocked(getAppInfo).mockReturnValue(
      new Promise((_resolve, reject) => {
        rejectInfo = reject;
      }),
    );
    const { unmount } = render(<App />);
    unmount();
    await act(async () => rejectInfo(new Error('fixture failure')));
    expect(showMainWindow).not.toHaveBeenCalled();
  });

  it('reports native IPC details and keeps unavailable playback disabled', async () => {
    render(<App />);
    expect(await screen.findByText('v0.1.0 / windows')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Play' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Next track' })).toBeDisabled();
    expect(screen.getByText('Synthetic preview')).toBeInTheDocument();
    expect(screen.getByText('Stream quality: unavailable')).toBeInTheDocument();
  });

  it('supports keyboard selection and playlist filtering', async () => {
    const user = userEvent.setup();
    render(<App />);
    const playlist = screen.getByRole('button', {
      name: /Soft machinery.*Static Gardens/,
    });
    playlist.focus();
    await user.keyboard('{Enter}');
    expect(
      screen.getByRole('heading', { name: 'Soft machinery' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /Signal at dawn.*Glass Meridian/ }),
    ).not.toBeInTheDocument();
    const track = screen.getByRole('button', {
      name: /Amber lines.*Static Gardens/,
    });
    track.focus();
    await user.keyboard('{Enter}');
    expect(
      screen.getByRole('heading', { name: 'Amber lines' }),
    ).toBeInTheDocument();
  });

  it('cycles preview modes by keyboard and retains them across tracks and navigation', async () => {
    const user = userEvent.setup();
    const { unmount } = render(<App />);
    await screen.findByText('v0.1.0 / windows');
    const controls = screen.getByRole('group', {
      name: 'Preview player controls',
    });
    expect(
      within(controls)
        .getAllByRole('button')
        .map((button) => button.getAttribute('aria-label')),
    ).toEqual([
      'Shuffle: Off (preview)',
      'Previous track',
      'Play',
      'Next track',
      'Repeat: Off (preview)',
    ]);
    const shuffle = screen.getByRole('button', {
      name: 'Shuffle: Off (preview)',
    });
    expect(shuffle).toHaveAccessibleDescription(
      /track order and audio are unchanged/i,
    );
    shuffle.focus();
    await user.keyboard(' ');
    expect(shuffle).toHaveAccessibleName('Shuffle: On (preview)');
    expect(shuffle).toHaveAttribute('aria-pressed', 'true');
    await user.keyboard('{Tab}');
    const repeat = screen.getByRole('button', {
      name: 'Repeat: Off (preview)',
    });
    expect(repeat).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(repeat).toHaveAccessibleName('Repeat: All (preview)');
    expect(repeat).toHaveAttribute('aria-pressed', 'true');
    await user.keyboard(' ');
    expect(repeat).toHaveAccessibleName('Repeat: Song (preview)');
    expect(within(repeat).getByText('1')).toHaveAttribute(
      'aria-hidden',
      'true',
    );
    await user.click(
      screen.getByRole('button', { name: /Amber lines.*Static Gardens/ }),
    );
    await user.click(screen.getByRole('button', { name: /Search/ }));
    await user.click(screen.getByRole('button', { name: /Settings/ }));
    await user.click(
      screen.getByRole('button', { name: /Soft machinery.*Static Gardens/ }),
    );
    expect(
      screen.getByRole('heading', { name: 'Amber lines' }),
    ).toBeInTheDocument();
    expect(shuffle).toHaveAccessibleName('Shuffle: On (preview)');
    expect(repeat).toHaveAccessibleName('Repeat: Song (preview)');
    expect(
      screen
        .getAllByRole('button', { name: /Static Gardens/ })
        .slice(1)
        .map((button) => button.textContent),
    ).toEqual([
      'Night geometryStatic Gardens',
      'Amber linesStatic Gardens',
      'Small hoursStatic Gardens',
      'Paper satellitesStatic Gardens',
    ]);
    await user.click(repeat);
    expect(repeat).toHaveAccessibleName('Repeat: Off (preview)');
    expect(repeat).toHaveAttribute('aria-pressed', 'false');
    expect(within(repeat).queryByText('1')).not.toBeInTheDocument();
    await user.click(shuffle);
    expect(shuffle).toHaveAttribute('aria-pressed', 'false');
    await user.click(shuffle);
    await user.click(repeat);
    expect(savePreferences).not.toHaveBeenCalled();
    unmount();
    render(<App />);
    expect(
      screen.getByRole('button', { name: 'Shuffle: Off (preview)' }),
    ).toHaveAttribute('aria-pressed', 'false');
    expect(
      screen.getByRole('button', { name: 'Repeat: Off (preview)' }),
    ).toHaveAttribute('aria-pressed', 'false');
  });

  it('searches synthetic data and announces an empty result', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: /Search/ }));
    const search = screen.getByRole('searchbox');
    expect(search).toHaveFocus();
    await user.type(search, 'not in preview');
    expect(screen.getByText(/No preview tracks match/)).toBeInTheDocument();
  });

  it('restores display settings and reports failed persistence', async () => {
    const user = userEvent.setup();
    vi.mocked(getAppInfo).mockResolvedValue({
      version: '0.1.0',
      platform: 'windows',
      preferences: { scale: 125, comfortable: true, volume: 40 },
      storageError: null,
    });
    vi.mocked(savePreferences).mockRejectedValue(new Error('fixture failure'));
    render(<App />);
    await screen.findByText('v0.1.0 / windows');
    await user.click(screen.getByRole('button', { name: /Settings/ }));
    expect(screen.getByRole('combobox')).toHaveValue('125');
    expect(screen.getByRole('checkbox')).toBeChecked();
    await user.selectOptions(screen.getByRole('combobox'), '200');
    await waitFor(() =>
      expect(document.documentElement.style.fontSize).toBe('200%'),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Preferences cannot be saved',
    );
  });
});
