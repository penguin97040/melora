import { describeFailure, isOraError } from '../features/account';
import { countLabel, trackTime } from '../features/library';
import type { Queue } from '../features/library';
import { releaseDate } from '../features/search';
import type { TrackRow } from '../native/client';
import type { Feature } from '../native/bindings/Feature';

type Props = {
  queue: Queue;
  /** The device playing, for the intro; `null` when nothing is active. */
  device: string | null;
  onAllow: (feature: Feature) => void;
};

function QueueTable({
  label,
  rows,
  current,
}: {
  label: string;
  rows: TrackRow[];
  current: boolean;
}) {
  return (
    <div className="track-table-wrapper">
      <table className="track-table">
        <caption className="sr-only">{label}</caption>
        <thead>
          <tr>
            <th scope="col">#</th>
            <th scope="col">Track / artist</th>
            <th scope="col">Album</th>
            <th scope="col">Time</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const episode = row.uri?.startsWith('spotify:episode:') === true;
            return (
              <tr
                key={row.position}
                className={current ? 'selected-track' : ''}
              >
                <td className="track-number">
                  {current ? '▶' : String(row.position + 1).padStart(2, '0')}
                </td>
                <td>
                  <span className="track-select">
                    <span>{row.name || 'Unknown title'}</span>
                    <span className="track-artist">
                      {episode
                        ? row.album || 'Podcast episode'
                        : row.artists.join(', ')}
                    </span>
                  </span>
                </td>
                <td className="album-cell">
                  {episode ? releaseDate(row.releaseDate) : row.album}
                </td>
                <td className="duration">{trackTime(row.durationMs)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** The queue in the centre: what plays now, then what Spotify plays next. */
export function SpotifyQueue({ queue, device, onAllow }: Props) {
  const { view, loading, error } = queue;
  const needs =
    isOraError(error) && error.kind === 'needsScope' ? error.feature : null;
  const next = view?.next ?? [];

  return (
    <>
      <section className="collection-heading">
        <p className="eyebrow">Spotify playback</p>
        <h1>Queue</h1>
        <p className="intro">
          {device
            ? `What plays next on ${device}.`
            : 'What plays next on your active Spotify device.'}{' '}
          Right-click a song or episode anywhere in Melora to add it.
        </p>
        <div className="collection-meta">
          <span>{countLabel(next.length, 'item')} next</span>
          <span>From Spotify</span>
          <span>
            <button className="link-button" onClick={queue.refresh}>
              Refresh
            </button>
          </span>
        </div>
      </section>
      <div className="content-note">
        <span className="note-mark">i</span>
        <p>Spotify doesn’t allow reordering the queue from other apps.</p>
      </div>
      {error ? (
        <p className="error-message" role="alert">
          {describeFailure(error)}{' '}
          {needs ? (
            <button className="link-button" onClick={() => onAllow(needs)}>
              Allow
            </button>
          ) : (
            <button className="link-button" onClick={queue.refresh}>
              Try again
            </button>
          )}
        </p>
      ) : view === null ? (
        loading && (
          <p className="empty-state" role="status">
            Loading from Spotify…
          </p>
        )
      ) : (
        <div aria-busy={loading}>
          <section aria-label="Now playing">
            <div className="results-heading">
              <h2 className="eyebrow">Now playing</h2>
            </div>
            {view.current ? (
              <QueueTable label="Playing now" rows={[view.current]} current />
            ) : (
              <p className="empty-state" role="status">
                Nothing is playing. Start something to see what plays next.
              </p>
            )}
          </section>
          <section aria-label="Next up">
            <div className="results-heading">
              <h2 className="eyebrow">Next up</h2>
            </div>
            {next.length > 0 ? (
              <QueueTable label="Next up" rows={next} current={false} />
            ) : (
              view.current && (
                <p className="empty-state" role="status">
                  Nothing is queued after this.
                </p>
              )
            )}
          </section>
        </div>
      )}
    </>
  );
}
