import { countLabel, useEndOfList, useWindow } from '../features/library';
import type { CatalogSource, Paged } from '../features/library';
import type { CatalogItem } from '../native/client';
import type { Feature } from '../native/bindings/Feature';
import { ListFooter, Spacer } from './SpotifyList';

type Props = {
  source: CatalogSource;
  list: Paged<CatalogItem>;
  /** Opens an album's tracks or a show's episodes. */
  onOpenItem: (item: CatalogItem) => void;
  onAllow: (feature: Feature) => void;
};

/** Saved albums or saved shows; choosing one opens it. */
export function SpotifyCatalog({ source, list, onOpenItem, onAllow }: Props) {
  const albums = source.kind === 'albums';
  const noun = albums ? 'album' : 'podcast';
  const end = useEndOfList(list);
  const [body, rows] = useWindow(list.items.length);

  return (
    <>
      <section className="collection-heading">
        <p className="eyebrow">Your Spotify library</p>
        <h1>{albums ? 'Albums' : 'Podcasts'}</h1>
        <p className="intro">
          {albums
            ? 'Albums you’ve saved on Spotify, newest first.'
            : 'Podcasts you follow on Spotify.'}{' '}
          Select one to see its {albums ? 'tracks' : 'episodes'}.
        </p>
        <div className="collection-meta">
          <span>{countLabel(list.total, noun)}</span>
          <span>From Spotify</span>
        </div>
      </section>
      <div className="track-table-wrapper">
        <table className="track-table">
          <caption className="sr-only">
            {albums ? 'Saved albums' : 'Saved podcasts'} from Spotify. Select
            one to open it.
          </caption>
          <thead>
            <tr>
              <th scope="col">#</th>
              <th scope="col">{albums ? 'Album / artist' : 'Podcast'}</th>
              <th scope="col">{albums ? 'Released' : ''}</th>
              <th scope="col">{albums ? 'Tracks' : 'Episodes'}</th>
            </tr>
          </thead>
          <tbody ref={body}>
            <Spacer rows={rows.start} height={rows.rowHeight} />
            {list.items.slice(rows.start, rows.end).map((item) => (
              <tr key={item.position}>
                <td className="track-number">
                  {String(item.position + 1).padStart(2, '0')}
                </td>
                <td>
                  <button
                    className="track-select"
                    onClick={() => onOpenItem(item)}
                  >
                    <span>{item.name || 'Untitled'}</span>
                    <span className="track-artist">{item.by.join(', ')}</span>
                  </button>
                </td>
                <td className="album-cell">
                  {albums ? item.releaseDate.slice(0, 4) : ''}
                </td>
                <td className="duration">
                  {item.total === null
                    ? ''
                    : item.total.toLocaleString('en-NZ')}
                </td>
              </tr>
            ))}
            <Spacer
              rows={list.items.length - rows.end}
              height={rows.rowHeight}
            />
          </tbody>
        </table>
      </div>
      <ListFooter
        list={list}
        noun={noun}
        empty={
          albums
            ? 'No saved albums yet. Save one with + on its page, or in Spotify.'
            : 'No podcasts yet. Follow one in Spotify, or save one from Search.'
        }
        onAllow={onAllow}
        end={end}
      />
    </>
  );
}
