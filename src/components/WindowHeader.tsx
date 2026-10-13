import { useEffect, useRef, useState } from 'react';
import brandIcon from '../../src-tauri/icons/source.svg';
import {
  closeWindow,
  doubleClickWindowHeader,
  isWindowMaximised,
  minimiseWindow,
  onWindowResize,
  startWindowResize,
  startWindowDragging,
  toggleWindowMaximise,
} from '../native/client';
import type { ResizeEdge, WindowChrome } from '../native/client';

const edges: ResizeEdge[] = [
  'North',
  'South',
  'East',
  'West',
  'NorthEast',
  'NorthWest',
  'SouthEast',
  'SouthWest',
];

export function WindowHeader({
  chrome,
  onError,
}: {
  chrome: WindowChrome;
  onError: (message: string) => void;
}) {
  const [maximised, setMaximised] = useState(false);
  const doubleClickPosition = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (chrome !== 'custom') return;
    let active = true;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    async function sync() {
      const request = ++revision;
      try {
        const value = await isWindowMaximised();
        if (active && request === revision) setMaximised(value);
      } catch {
        if (active && request === revision)
          onError('The window state could not be read.');
      }
    }
    // Subscribe before querying so a native resize cannot fall into a startup gap.
    void onWindowResize(() => void sync())
      .then((stop) => {
        if (!active) stop();
        else {
          unlisten = stop;
          void sync();
        }
      })
      .catch(() => {
        if (active) {
          onError('Window changes could not be monitored.');
          void sync();
        }
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [chrome, onError]);

  async function operate(operation: () => Promise<void>) {
    try {
      await operation();
    } catch {
      onError('The window operation failed. Try again.');
    }
  }

  return (
    <>
      <header
        className="masthead"
        data-tauri-drag-region={chrome === 'preview' ? 'false' : 'deep'}
        onMouseDown={(event) => {
          if (
            chrome === 'preview' ||
            event.button !== 0 ||
            (event.target instanceof Element &&
              event.target.closest('[data-tauri-drag-region="false"]'))
          )
            return;
          // Manual Tauri drag-region handling lets operation failures reach the UI.
          // Stop its document handler to avoid issuing the same command twice.
          event.stopPropagation();
          event.preventDefault();
          if (event.detail === 2 && chrome === 'macos') {
            doubleClickPosition.current = {
              x: event.clientX,
              y: event.clientY,
            };
          } else if (event.detail === 1 || event.detail === 2) {
            void operate(
              event.detail === 2
                ? doubleClickWindowHeader
                : startWindowDragging,
            );
          }
        }}
        onMouseUp={(event) => {
          const position = doubleClickPosition.current;
          doubleClickPosition.current = null;
          if (chrome !== 'macos' || event.detail !== 2 || !position) return;
          event.stopPropagation();
          if (
            event.button === 0 &&
            event.clientX === position.x &&
            event.clientY === position.y &&
            !(
              event.target instanceof Element &&
              event.target.closest('[data-tauri-drag-region="false"]')
            )
          )
            void operate(doubleClickWindowHeader);
        }}
      >
        <div className="brand">
          <img className="brand-symbol" src={brandIcon} alt="" />
          <span>melora</span>
        </div>
        {chrome !== 'macos' && (
          <div
            className="window-controls"
            role="group"
            aria-label="Window controls"
            data-tauri-drag-region="false"
          >
            <button
              aria-label="Minimise window"
              disabled={chrome === 'preview'}
              onClick={() => void operate(minimiseWindow)}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M2 11h12" />
              </svg>
            </button>
            <button
              aria-label={maximised ? 'Restore window' : 'Maximise window'}
              disabled={chrome === 'preview'}
              onClick={() => void operate(toggleWindowMaximise)}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                {maximised ? (
                  <path d="M5 5V2h9v9h-3M2 5h9v9H2z" />
                ) : (
                  <path d="M2 2h12v12H2z" />
                )}
              </svg>
            </button>
            <button
              className="window-close"
              aria-label="Close window"
              disabled={chrome === 'preview'}
              onClick={() => void operate(closeWindow)}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="m3 3 10 10M13 3 3 13" />
              </svg>
            </button>
          </div>
        )}
      </header>
      {chrome === 'custom' &&
        !maximised &&
        edges.map((edge) => (
          <div
            key={edge}
            className={`resize-target resize-${edge.toLowerCase()}`}
            data-resize-edge={edge}
            data-tauri-drag-region="false"
            aria-hidden="true"
            onMouseDown={(event) => {
              if (event.button !== 0) return;
              event.preventDefault();
              void operate(() => startWindowResize(edge));
            }}
          />
        ))}
    </>
  );
}
