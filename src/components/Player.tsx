import type { PreviewTrack } from '../preview';

export function Player({ track }: { track: PreviewTrack }) {
  return (
    <aside className="player panel" aria-label="Player preview">
      <div className="panel-label">
        <span>Now playing</span>
        <span>Preview</span>
      </div>
      <div className="player-body">
        <div
          className="cover"
          role="img"
          aria-label="Abstract synthetic preview artwork"
        >
          <div className="cover-orbit" />
          <span className="cover-mark">m.</span>
          <span className="cover-caption">AFTER THE QUIET / 001</span>
        </div>
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
        <div className="transport" aria-label="Playback controls">
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
        </div>
        <div className="device">
          <span className="eyebrow">Playback device</span>
          <p>No device connected</p>
        </div>
        <p className="player-footnote">
          Spotify sign-in and playback arrive in later milestones. These tracks
          and artwork are synthetic.
        </p>
      </div>
    </aside>
  );
}
