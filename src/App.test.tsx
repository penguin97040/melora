import { act, render, screen, waitFor, within } from '@testing-library/react';
import { StrictMode } from 'react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import App, { LIKE_MESSAGE_MS, REMOTE_REFRESH_MS } from './App';
import {
  allowLibrary,
  enablePlayback,
  getAppInfo,
  getWindowChrome,
  refreshPlayback,
  savePreferences,
  showMainWindow,
  refreshLiked,
  subscribePlayback,
  subscribeSpotify,
  toggleLiked,
} from './native/client';
import type { AppInfo, SpotifyStatus } from './native/client';
import type { Feature } from './native/bindings/Feature';
import { playbackStatus } from './native/playbackFixture';

vi.mock('./native/client', () => ({
  defaultPreferences: { scale: 100, comfortable: false, volume: 70 },
  getAppInfo: vi.fn(),
  getWindowChrome: vi.fn(),
  savePreferences: vi.fn(),
  showMainWindow: vi.fn(),
  hasNativeStorage: () => true,
  subscribeSpotify: vi.fn(async (onStatus: (status: unknown) => void) =>
    onStatus({
      auth: { kind: 'notConfigured' },
      clientIdFromEnvironment: false,
      clientIdHint: null,
      lastError: null,
      redirectUri: 'http://127.0.0.1:43821/callback',
    }),
  ),
  subscribePlayback: vi.fn(async (onStatus: (status: unknown) => void) => {
    const { playbackStatus } = await import('./native/playbackFixture');
    onStatus(playbackStatus());
  }),
  openInSpotify: vi.fn(),
  setSpotifyClientId: vi.fn(),
  signInWithSpotify: vi.fn(),
  cancelSpotifySignIn: vi.fn(),
  signOutOfSpotify: vi.fn(),
  activatePlayback: vi.fn(),
  allowPlayback: vi.fn(),
  enablePlayback: vi.fn(),
  playHere: vi.fn(),
  sendPlaybackCommand: vi.fn(),
  refreshPlayback: vi.fn(),
  listDevices: vi.fn(),
  transferPlayback: vi.fn(),
  allowDeviceControl: vi.fn(),
  allowLibrary: vi.fn(),
  refreshLiked: vi.fn(),
  toggleLiked: vi.fn(),
}));

function signedIn(granted: Feature[]): SpotifyStatus {
  return {
    auth: { kind: 'signedIn', profile: null, sessionOnly: false, granted },
    clientIdFromEnvironment: false,
    clientIdHint: null,
    lastError: null,
    redirectUri: 'http://127.0.0.1:43821/callback',
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getWindowChrome).mockResolvedValue('preview');
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.5.3',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
  vi.mocked(showMainWindow).mockResolvedValue(undefined);
  vi.mocked(refreshPlayback).mockResolvedValue(undefined);
  vi.mocked(refreshLiked).mockResolvedValue(undefined);
  vi.mocked(enablePlayback).mockResolvedValue(true);
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
      expect(screen.getByText('v0.5.3 / windows')).toBeInTheDocument();
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
        version: '0.5.3',
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

  it('waits for native chrome independently of preferences before revealing', async () => {
    let resolveChrome!: (mode: 'macos') => void;
    vi.mocked(getWindowChrome).mockReturnValue(
      new Promise((resolve) => {
        resolveChrome = resolve;
      }),
    );
    render(<App />);
    await screen.findByText('v0.5.3 / windows');
    expect(showMainWindow).not.toHaveBeenCalled();
    await act(async () => resolveChrome('macos'));
    expect(showMainWindow).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole('group', { name: 'Window controls' }),
    ).not.toBeInTheDocument();
  });

  it('reveals a usable shell with an error if chrome detection fails', async () => {
    vi.mocked(getWindowChrome).mockRejectedValue(
      new Error('synthetic failure'),
    );
    render(<App />);
    await waitFor(() => expect(showMainWindow).toHaveBeenCalledTimes(1));
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Window controls could not be loaded',
    );
    expect(screen.getByRole('button', { name: 'Close window' })).toBeDisabled();
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
    expect(await screen.findByText('v0.5.3 / windows')).toBeInTheDocument();
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
    await screen.findByText('v0.5.3 / windows');
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
      version: '0.5.3',
      platform: 'windows',
      preferences: { scale: 125, comfortable: true, volume: 40 },
      storageError: null,
    });
    vi.mocked(savePreferences).mockRejectedValue(new Error('fixture failure'));
    render(<App />);
    await screen.findByText('v0.5.3 / windows');
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
  it('searches all preview tracks after a playlist selection and reports actual artist counts', async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText('v0.5.3 / windows');
    await user.click(
      screen.getByRole('button', { name: /Soft machinery.*Static Gardens/ }),
    );
    expect(screen.getByText('1 synthetic artist')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /Search/ }));
    await user.type(screen.getByRole('searchbox'), '  SIGNAL  ');
    expect(
      screen.getByRole('button', { name: /Signal at dawn.*Glass Meridian/ }),
    ).toBeInTheDocument();
    expect(screen.getByText('1 synthetic artist')).toBeInTheDocument();
    await user.clear(screen.getByRole('searchbox'));
    expect(screen.getByText('12 tracks')).toBeInTheDocument();
    expect(screen.getByText('3 synthetic artists')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /Library/ }));
    expect(
      screen.getByRole('heading', { name: 'Soft machinery' }),
    ).toBeInTheDocument();
    expect(screen.getByText('4 tracks')).toBeInTheDocument();
  });

  it('serialises rapid preference changes and recovers after a failed write', async () => {
    const user = userEvent.setup();
    let rejectFirst!: (error: Error) => void;
    vi.mocked(savePreferences).mockImplementationOnce(
      () =>
        new Promise((_resolve, reject) => {
          rejectFirst = reject;
        }),
    );
    render(<App />);
    await screen.findByText('v0.5.3 / windows');
    await user.click(screen.getByRole('button', { name: /Settings/ }));
    await user.selectOptions(screen.getByRole('combobox'), '125');
    await user.click(screen.getByRole('checkbox'));
    await user.selectOptions(screen.getByRole('combobox'), '150');
    expect(savePreferences).toHaveBeenCalledTimes(1);
    await act(async () => rejectFirst(new Error('synthetic storage failure')));
    await waitFor(() => expect(savePreferences).toHaveBeenCalledTimes(3));
    expect(
      vi.mocked(savePreferences).mock.calls.map(([value]) => value),
    ).toEqual([
      { scale: 125, comfortable: false, volume: 70 },
      { scale: 125, comfortable: true, volume: 70 },
      { scale: 150, comfortable: true, volume: 70 },
    ]);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});

describe('playback device footer', () => {
  it('names this app only while Melora is the active Spotify device', async () => {
    vi.mocked(subscribePlayback).mockImplementationOnce(async (onStatus) =>
      onStatus(playbackStatus({ tier: 'active', paused: false })),
    );
    render(<App />);
    expect(await screen.findByText('This app')).toBeInTheDocument();
    expect(screen.queryByText('No device connected')).not.toBeInTheDocument();
  });

  it('names another device while Melora controls it', async () => {
    vi.mocked(subscribePlayback).mockImplementationOnce(async (onStatus) =>
      onStatus(
        playbackStatus({
          tier: 'ready',
          paused: false,
          device: {
            key: '0123456789abcdef',
            name: 'Kitchen',
            kind: 'speaker',
            active: true,
            restricted: false,
            thisApp: false,
            supportsVolume: true,
            volumePercent: 40,
          },
        }),
      ),
    );
    render(<App />);
    const footer = await screen.findByRole('contentinfo');
    await waitFor(() =>
      expect(within(footer).getByText('Kitchen')).toBeInTheDocument(),
    );
    expect(screen.getByText('Playing on Kitchen')).toBeInTheDocument();
  });
});

describe('device control', () => {
  it('reads the active device on sign-in, on focus and at a low rate', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      vi.mocked(subscribeSpotify).mockImplementationOnce(async (onStatus) =>
        onStatus(signedIn(['SignIn', 'ConnectControl'])),
      );
      render(<App />);
      await waitFor(() => expect(refreshPlayback).toHaveBeenCalledTimes(1));
      act(() => {
        window.dispatchEvent(new Event('focus'));
      });
      expect(refreshPlayback).toHaveBeenCalledTimes(2);
      await act(async () => {
        vi.advanceTimersByTime(REMOTE_REFRESH_MS);
      });
      expect(refreshPlayback).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it('never reads devices without the permission', async () => {
    render(<App />);
    await screen.findByText('Set up Spotify');
    expect(refreshPlayback).not.toHaveBeenCalled();
  });

  it('turns Melora’s playback on at launch once it is allowed', async () => {
    // Strict Mode subscribes twice.
    const allowed = signedIn(['SignIn', 'EmbeddedPlayback', 'ConnectControl']);
    vi.mocked(subscribeSpotify)
      .mockImplementationOnce(async (onStatus) => onStatus(allowed))
      .mockImplementationOnce(async (onStatus) => onStatus(allowed));
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await waitFor(() => expect(enablePlayback).toHaveBeenCalledTimes(1));
    expect(enablePlayback).toHaveBeenCalledWith(0.7);
  });
});

describe('Liked Songs', () => {
  const playing = () =>
    playbackStatus({
      tier: 'active',
      paused: false,
      durationMs: 180_000,
      updatedAtMs: Date.now(),
      nowPlaying: {
        name: 'Song',
        artists: ['Artist'],
        album: 'Album',
        spotifyUrl: 'https://open.spotify.com/track/x',
        imageUrl: null,
        liked: null,
      },
    });
  const playbackOnly = ['SignIn', 'EmbeddedPlayback'] as Feature[];

  it('asks for library access on the first +, then adds the track and says so', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      vi.mocked(subscribeSpotify).mockImplementationOnce(async (onStatus) =>
        onStatus(signedIn(playbackOnly)),
      );
      vi.mocked(subscribePlayback).mockImplementationOnce(async (onStatus) =>
        onStatus(playing()),
      );
      vi.mocked(allowLibrary).mockResolvedValue(
        signedIn([...playbackOnly, 'LibrarySaveRemove']),
      );
      vi.mocked(toggleLiked).mockResolvedValue(true);
      render(<App />);
      const add = await screen.findByRole('button', {
        name: 'Add to Liked Songs',
      });
      // Without the permission, nothing about the library is read.
      expect(refreshLiked).not.toHaveBeenCalled();
      await userEvent
        .setup({ advanceTimers: vi.advanceTimersByTime })
        .click(add);
      expect(await screen.findByText('Added to Liked Songs')).toBeVisible();
      expect(allowLibrary).toHaveBeenCalledTimes(1);
      expect(toggleLiked).toHaveBeenCalledTimes(1);
      await act(async () => {
        vi.advanceTimersByTime(LIKE_MESSAGE_MS);
      });
      expect(
        screen.queryByText('Added to Liked Songs'),
      ).not.toBeInTheDocument();
      expect(screen.getByText(/Shuffle: Off · Repeat: Off/)).toBeVisible();
    } finally {
      vi.useRealTimers();
    }
  });

  it('does nothing more when the permission is declined', async () => {
    vi.mocked(subscribeSpotify).mockImplementationOnce(async (onStatus) =>
      onStatus(signedIn(playbackOnly)),
    );
    vi.mocked(subscribePlayback).mockImplementationOnce(async (onStatus) =>
      onStatus(playing()),
    );
    vi.mocked(allowLibrary).mockResolvedValue(signedIn(playbackOnly));
    render(<App />);
    await userEvent.click(
      await screen.findByRole('button', { name: 'Add to Liked Songs' }),
    );
    await waitFor(() => expect(allowLibrary).toHaveBeenCalledTimes(1));
    expect(toggleLiked).not.toHaveBeenCalled();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('checks each track once, and again on focus', async () => {
    vi.mocked(subscribeSpotify).mockImplementationOnce(async (onStatus) =>
      onStatus(signedIn([...playbackOnly, 'LibrarySaveRemove'])),
    );
    vi.mocked(subscribePlayback).mockImplementationOnce(async (onStatus) =>
      onStatus(playing()),
    );
    vi.mocked(toggleLiked).mockResolvedValue(false);
    render(<App />);
    await waitFor(() => expect(refreshLiked).toHaveBeenCalledTimes(1));
    act(() => {
      window.dispatchEvent(new Event('focus'));
    });
    expect(refreshLiked).toHaveBeenCalledTimes(2);
    await userEvent.click(
      screen.getByRole('button', { name: 'Add to Liked Songs' }),
    );
    expect(await screen.findByText('Removed from Liked Songs')).toBeVisible();
    expect(allowLibrary).not.toHaveBeenCalled();
  });
});
