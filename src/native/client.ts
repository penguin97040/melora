import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type { OraError } from './bindings/OraError';
import type { Profile } from './bindings/Profile';
import type { Status } from './bindings/Status';

export type { OraError, Profile };
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

// Spotify account commands from the shared `ora` plugin. Tokens never reach JavaScript; these
// calls only move status, the Client ID and validated open.spotify.com links.
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

export async function cancelSpotifySignIn(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|cancel_sign_in');
}

export async function signOutOfSpotify(): Promise<SpotifyStatus> {
  requireDesktop();
  return invoke<SpotifyStatus>('plugin:ora|sign_out');
}

/** Opens an open.spotify.com page in the system browser (validated again in Rust). */
export async function openInSpotify(url: string): Promise<void> {
  requireDesktop();
  return invoke<void>('plugin:ora|open_in_spotify', { url });
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
