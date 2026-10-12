import { invoke, isTauri } from '@tauri-apps/api/core';

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
