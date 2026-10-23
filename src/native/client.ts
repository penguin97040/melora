import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { install } from '../../ora/sdk-bridge/sdk-bridge.js';
import type { Bridge } from '../../ora/sdk-bridge/sdk-bridge.js';
import type { Device } from './bindings/Device';
import type { DeviceKind } from './bindings/DeviceKind';
import type { OraError } from './bindings/OraError';
import type { PlaybackStatus } from './bindings/PlaybackStatus';
import type { PlayerCommand } from './bindings/PlayerCommand';
import type { Profile } from './bindings/Profile';
import type { Status } from './bindings/Status';

export type {
  Device,
  DeviceKind,
  OraError,
  PlaybackStatus,
  PlayerCommand,
  Profile,
};
export type SpotifyStatus = Status;

export type Preferences = {
  scale: number;
  comfortable: boolean;
  volume: number;
};
export type AppInfo = {
  version: string;
  platform: string;
  preferences: Preferences;
  storageError: string | null;
};
export const defaultPreferences: Preferences = {
  scale: 100,
  comfortable: false,
  volume: 70,
};

export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) {
    return {
      version: '0.1.0',
      platform: 'Browser preview',
      preferences: defaultPreferences,
      storageError: null,
    };
  }
  return invoke<AppInfo>('app_info');
}

export async function savePreferences(
  preferences: Preferences,
): Promise<Preferences> {
  if (!isTauri()) return preferences;
  return invoke<Preferences>('save_preferences', { preferences });
}

export function hasNativeStorage(): boolean {
  return isTauri();
}

export async function showMainWindow(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>('show_main_window');
}

// Spotify account commands from the shared `ora` plugin. These calls only move status, the
// Client ID and validated open.spotify.com links. The one exception is the shared sdk-bridge,
// which asks for an access token for the Web Playback SDK's callback and never keeps it.
const previewSpotifyStatus: SpotifyStatus = {
  auth: { kind: 'notConfigured' },
  clientIdFromEnvironment: false,
  clientIdHint: null,
  lastError: null,
  redirectUri: 'http://127.0.0.1:43821/callback',
};

function requireDesktop(): void {
  if (!isTauri()) throw new Error('Spotify sign-in needs the desktop app.');
}

/** Calls `onStatus` with the current status and then every change. */
export async function subscribeSpotify(
  onStatus: (status: SpotifyStatus) => void,
): Promise<void> {
  if (!isTauri()) {
    onStatus(previewSpotifyStatus);
    return;
  }
  const channel = new Channel<SpotifyStatus>();
  channel.onmessage = onStatus;
  await invoke<void>('plugin:ora|subscribe', { onStatus: channel });
}

export async function setSpotifyClientId(
  clientId: string,
): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|set_client_id', { clientId });
}

export async function signInWithSpotify(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|sign_in');
}

/** Signs in again, asking Spotify for the extra permission embedded playback needs. */
export async function allowPlayback(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|sign_in', {
    feature: 'EmbeddedPlayback',
  });
}

/** Signs in again, asking Spotify for permission to see and control the account's devices. */
export async function allowDeviceControl(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|sign_in', {
    feature: 'ConnectControl',
  });
}

/** Signs in again, asking Spotify for permission to read and change the account's library. */
export async function allowLibrary(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|sign_in', {
    feature: 'LibrarySaveRemove',
  });
}

export async function cancelSpotifySignIn(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|cancel_sign_in');
}

export async function signOutOfSpotify(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|sign_out');
}

/** Loads the profile if it's missing; the status update carries the result too. */
export async function loadSpotifyProfile(): Promise<Profile | null> {
  requireDesktop();
  return invoke<Profile | null>('plugin:ora|profile');
}

/** Opens an open.spotify.com page in the system browser (validated again in Rust). */
export async function openInSpotify(url: string): Promise<void> {
  requireDesktop();
  return invoke<void>('plugin:ora|open_in_spotify', { url });
}

// Embedded playback (Phase 3) through the shared Web Playback SDK bridge, and control of other
// Spotify devices (Phase 4) through the Web API in Rust.
const previewPlaybackStatus: PlaybackStatus = {
  tier: 'off',
  device: null,
  nowPlaying: null,
  paused: true,
  positionMs: 0,
  durationMs: 0,
  updatedAtMs: 0,
  shuffle: false,
  repeat: 'off',
  disallows: {
    pause: false,
    resume: false,
    next: false,
    previous: false,
    seek: false,
  },
  issue: null,
  probe: null,
  cspBlocks: [],
};
let bridge: Bridge | null = null;

function playbackBridge(volume: number): Bridge {
  requireDesktop();
  bridge ??= install({ invoke, Channel, name: 'Melora', volume });
  return bridge;
}

/** Calls `onStatus` with the current playback status and then every change. */
export async function subscribePlayback(
  onStatus: (status: PlaybackStatus) => void,
): Promise<void> {
  if (!isTauri()) {
    onStatus(previewPlaybackStatus);
    return;
  }
  const channel = new Channel<PlaybackStatus>();
  channel.onmessage = onStatus;
  await invoke<void>('plugin:ora|subscribe_playback', { onStatus: channel });
}

/** Checks DRM support, then loads Spotify's player. Resolves whether Melora became a device. */
export async function enablePlayback(volume: number): Promise<boolean> {
  return playbackBridge(volume).enable();
}

/** Must run synchronously inside the click that starts playback. */
export function activatePlayback(): void {
  bridge?.activate();
}

/** Plays the pasted Spotify link here, or moves what was last playing to Melora. */
export async function playHere(link: string): Promise<void> {
  requireDesktop();
  const trimmed = link.trim();
  return invoke<void>('plugin:ora|play_here', {
    link: trimmed === '' ? null : trimmed,
  });
}

export async function sendPlaybackCommand(
  command: PlayerCommand,
): Promise<void> {
  requireDesktop();
  return invoke<void>('plugin:ora|playback_command', { command });
}

/** Reads which device is active and what it plays. The browser preview has none. */
export async function refreshPlayback(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>('plugin:ora|refresh_playback');
}

/** The account's Spotify devices. Keys are opaque; device IDs stay in Rust. */
export async function listDevices(): Promise<Device[]> {
  requireDesktop();
  return invoke<Device[]>('plugin:ora|list_devices');
}

/** Moves playback to the device behind a key from `listDevices`. */
export async function transferPlayback(key: string): Promise<void> {
  requireDesktop();
  return invoke<void>('plugin:ora|transfer_playback', { key });
}

/** Asks Spotify whether the track playing now is liked; the playback status carries it. */
export async function refreshLiked(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>('plugin:ora|refresh_liked');
}

/** The `+` control. Resolves whether the track playing now is liked afterwards. */
export async function toggleLiked(): Promise<boolean> {
  requireDesktop();
  return invoke<boolean>('plugin:ora|toggle_liked');
}

export type WindowChrome = 'custom' | 'macos' | 'preview';
export type ResizeEdge =
  | 'North'
  | 'South'
  | 'East'
  | 'West'
  | 'NorthEast'
  | 'NorthWest'
  | 'SouthEast'
  | 'SouthWest';

export async function getWindowChrome(): Promise<WindowChrome> {
  return isTauri() ? invoke<WindowChrome>('window_chrome') : 'preview';
}

async function mainWindow() {
  if (!isTauri()) throw new Error('Window controls require the desktop app.');
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  const window = getCurrentWindow();
  if (window.label !== 'main') throw new Error('Main window required.');
  return window;
}

export async function minimiseWindow(): Promise<void> {
  return (await mainWindow()).minimize();
}

export async function closeWindow(): Promise<void> {
  return (await mainWindow()).close();
}

export async function toggleWindowMaximise(): Promise<void> {
  return (await mainWindow()).toggleMaximize();
}

export async function isWindowMaximised(): Promise<boolean> {
  return (await mainWindow()).isMaximized();
}

export async function startWindowResize(edge: ResizeEdge): Promise<void> {
  return (await mainWindow()).startResizeDragging(edge);
}

export async function onWindowResize(handler: () => void): Promise<() => void> {
  return (await mainWindow()).onResized(handler);
}

export async function startWindowDragging(): Promise<void> {
  return (await mainWindow()).startDragging();
}

export async function doubleClickWindowHeader(): Promise<void> {
  await mainWindow();
  // Use Tauri's resizability-checked operation for title-bar double clicks.
  return invoke<void>('plugin:window|internal_toggle_maximize');
}
