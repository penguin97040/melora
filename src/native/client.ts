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
