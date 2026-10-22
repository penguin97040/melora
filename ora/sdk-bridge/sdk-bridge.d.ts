// Types for sdk-bridge.js, so TypeScript clients can install the shared bridge.

export interface BridgeOptions {
  /** The client's IPC call, for example `invoke` from `@tauri-apps/api/core`. */
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  /** The client's IPC channel class, for example `Channel` from `@tauri-apps/api/core`. */
  Channel: new () => { onmessage: (message: never) => void };
  /** The device name shown in Spotify's device list. */
  name: string;
  /** Initial volume, 0 to 1. */
  volume?: number;
  window?: Window & typeof globalThis;
}

export interface Bridge {
  /** Probes, loads the SDK and connects. Resolves whether this app became a device. */
  enable(): Promise<boolean>;
  /** Call synchronously from a click, before the first transfer or play. */
  activate(): void;
  disconnect(): void;
}

export function install(options: BridgeOptions): Bridge;
