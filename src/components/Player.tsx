import { useState } from 'react';
import type { PreviewTrack } from '../preview';

export function Player({ track }: { track: PreviewTrack }) {
  const [shuffle, setShuffle] = useState(false);
  const [repeat, setRepeat] = useState<'Off' | 'All' | 'Song'>('Off');

  return (
    <aside className="player panel" aria-label="Player preview">
      <div className="panel-label">
        <span>Now playing</span>
        <span>Preview</span>
      </div>
      <div
        className="cover"
        role="img"
        aria-label="Abstract synthetic preview artwork"
      >
        <div className="cover-orbit" />
        <span className="cover-mark">m.</span>
        <span className="cover-caption">AFTER THE QUIET / 001</span>
      </div>
      <div className="player-body">
        <p className="eyebrow">Selected preview track</p>
        <h2 className="track-title">{track.title}</h2>
        <p className="artist-name">{track.artist}</p>
        <p className="muted">{track.album}</p>
        <div className="playback-notice">
          <span className="status-dot" />
          Playback unavailable in preview
        </div>
        <label className="sr-only" htmlFor="seek">
          Track position
        </label>
        <input id="seek" type="range" min="0" max="100" value="0" disabled />
        <div className="time-labels">
          <span>0:00</span>
          <span>{track.duration}</span>
        </div>
        <div
          className="transport"
          role="group"
          aria-label="Preview player controls"
        >
          <button
            className={shuffle ? 'mode-active' : ''}
            aria-label={`Shuffle: ${shuffle ? 'On' : 'Off'} (preview)`}
            aria-pressed={shuffle}
            aria-describedby="preview-modes-note"
            onClick={() => setShuffle((current) => !current)}
          >
            <svg className="mode-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M3 6h3c5 0 7 12 12 12h3m-4-4 4 4-4 4M3 18h3c2 0 3-2 4-4m4-4c1-2 2-4 4-4h3m-4-4 4 4-4 4" />
            </svg>
          </button>
          <button disabled aria-label="Previous track">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M5 5h3v14H5zM19 5v14L8 12z" />
            </svg>
          </button>
          <button disabled className="play-button" aria-label="Play">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M7 4v16l13-8z" />
            </svg>
          </button>
          <button disabled aria-label="Next track">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M16 5h3v14h-3zM5 5v14l11-7z" />
            </svg>
          </button>
          <button
            className={repeat !== 'Off' ? 'mode-active' : ''}
            aria-label={`Repeat: ${repeat} (preview)`}
            aria-pressed={repeat !== 'Off'}
            aria-describedby="preview-modes-note"
            onClick={() =>
              setRepeat((current) =>
                current === 'Off' ? 'All' : current === 'All' ? 'Song' : 'Off',
              )
            }
          >
            <svg className="mode-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M4 10V8a3 3 0 0 1 3-3h13m-4-4 4 4-4 4M20 14v2a3 3 0 0 1-3 3H4m4-4-4 4 4 4" />
            </svg>
            {repeat === 'Song' && (
              <span className="repeat-one" aria-hidden="true">
                1
              </span>
            )}
          </button>
        </div>
        <p className="mode-status" role="status">
          Shuffle: {shuffle ? 'On' : 'Off'} · Repeat: {repeat}
        </p>
        <div className="device">
          <span className="eyebrow">Playback device</span>
          <p>No device connected</p>
        </div>
        <p className="player-footnote" id="preview-modes-note">
          Shuffle and repeat are preview selections only; track order and audio
          are unchanged. Modes reset on relaunch. Spotify sign-in and playback
          arrive in later milestones. These tracks and artwork are synthetic.
        </p>
      </div>
    </aside>
  );
}
