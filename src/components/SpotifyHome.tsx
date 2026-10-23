import { useEffect, useState } from 'react';
import type { MouseEvent, ReactNode } from 'react';
import { describeFailure } from '../features/account';
import {
  countLabel,
  fetchCatalog,
  fetchTracks,
  granted,
  trackTime,
  usePaged,
} from '../features/library';
import type { ListSource, Paged } from '../features/library';
import { deviceKindLabel, deviceName, isLive } from '../features/playback';
import { listDevices } from '../native/client';
import type {
  CatalogItem,
  Device,
  PlaybackStatus,
  PlaylistSummary,
  SpotifyStatus,
  TrackRow,
} from '../native/client';
import type { Feature } from '../native/bindings/Feature';
import { menuPoint } from '../features/menu';
import type { MenuPoint } from '../features/menu';

/** Rows each Home section shows; "See all" opens the whole list. */
export const HOME_ROWS = 5;

type Props = {
  spotify: SpotifyStatus | null;
  playback: PlaybackStatus | null;
  /** The sidebar's playlists, `null` until they're allowed. */
  playlists: Paged<PlaylistSummary> | null;
  /** The `open.spotify.com` page of what's playing now, to mark its row. */
  playingUrl: string | null;
  onOpen: (source: ListSource) => void;
  onOpenItem: (item: CatalogItem) => void;
  onPlayRecent: (rows: TrackRow[], row: TrackRow) => void;
  onMenu: (row: TrackRow, at: MenuPoint) => void;
  onAllow: (feature: Feature) => void;
  onOpenSettings: () => void;
};

type Row = {
  key: string;
  title: string;
  subtitle: string;
  detail: string;
  extra: string;
  playing?: boolean;
  onSelect?: (() => void) | undefined;
  onMenu?: ((event: MouseEvent<HTMLElement>) => void) | undefined;
};

function HomeTable({ label, rows }: { label: string; rows: Row[] }) {
  return (
    <div className="track-table-wrapper">
      <table className="track-table">
        <caption className="sr-only">{label}</caption>
        <tbody>
          {rows.map((row, i) => (
            <tr key={row.key} className={row.playing ? 'selected-track' : ''}>
              <td className="track-number">{String(i + 1).padStart(2, '0')}</td>
              <td>
                {row.onSelect ? (
                  <button
                    className="track-select"
                    aria-current={row.playing ? 'true' : undefined}
                    onClick={row.onSelect}
                    onContextMenu={row.onMenu}
                  >
                    <span>{row.title}</span>
                    <span className="track-artist">{row.subtitle}</span>
                  </button>
                ) : (
                  <span className="track-select">
                    <span>{row.title}</span>
                    <span className="track-artist">{row.subtitle}</span>
                  </span>
                )}
              </td>
              <td className="album-cell">{row.detail}</td>
              <td className="duration">{row.extra}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** A Home section: its heading (with "See all"), then its rows, a message or an Allow link. */
function Section({
  title,
  seeAll,
  children,
}: {
  title: string;
  seeAll?: { label: string; onClick: () => void } | null;
  children: ReactNode;
}) {
  return (
    <section aria-label={title}>
      <div className="results-heading">
        <h2 className="eyebrow">{title}</h2>
        {seeAll && (
          <button className="link-button" onClick={seeAll.onClick}>
            {seeAll.label}
          </button>
        )}
      </div>
      {children}
    </section>
  );
}

function Status({ children }: { children: ReactNode }) {
  return (
    <p className="empty-state" role="status">
      {children}
    </p>
  );
}

/** Loading, a failure or the empty message for a Home list; `null` once there are rows. */
function listState<T>(list: Paged<T>, empty: string): ReactNode {
  if (list.items.length > 0) return null;
  if (list.loading) return <Status>Loading from Spotify…</Status>;
  if (list.error)
    return (
      <p className="error-message" role="alert">
        {describeFailure(list.error)}{' '}
        <button className="link-button" onClick={list.retry}>
          Try again
        </button>
      </p>
    );
  return <Status>{empty}</Status>;
}

function AllowLink({
  text,
  feature,
  onAllow,
}: {
  text: string;
  feature: Feature;
  onAllow: (feature: Feature) => void;
}) {
  return (
    <p className="empty-state">
      {text}{' '}
      <button className="link-button" onClick={() => onAllow(feature)}>
        Allow
      </button>
    </p>
  );
}

/** The account's Spotify devices, read once when Home opens (and again on focus). */
function useDevices(enabled: boolean) {
  const [state, setState] = useState<{
    devices: Device[] | null;
    error: unknown;
  }>({ devices: null, error: null });
  const [reads, setReads] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    void listDevices().then(
      (devices) => {
        if (active) setState({ devices, error: null });
      },
      (error: unknown) => {
        if (active) setState((previous) => ({ ...previous, error }));
      },
    );
    const again = () => setReads((n) => n + 1);
    window.addEventListener('focus', again);
    return () => {
      active = false;
      window.removeEventListener('focus', again);
    };
  }, [enabled, reads]);
  return enabled ? state : { devices: null, error: null };
}

/**
 * Home: what's playing, what played recently, saved albums and podcasts, playlists and devices.
 * Each section needs its own permission; until it's given, the section offers to ask for it.
 */
export function SpotifyHome({
  spotify,
  playback,
  playlists,
  playingUrl,
  onOpen,
  onOpenItem,
  onPlayRecent,
  onMenu,
  onAllow,
  onOpenSettings,
}: Props) {
  const signedIn = spotify?.auth.kind === 'signedIn';
  const account =
    spotify?.auth.kind === 'signedIn' ? (spotify.auth.profile?.id ?? '') : '';
  const may = (feature: Feature) => granted(spotify, feature);
  // Keyed per account, so a new sign-in never shows the previous account's lists.
  const recent = usePaged(
    may('RecentlyPlayed') ? `recent:${account}` : null,
    fetchTracks,
  );
  const albums = usePaged(
    may('SavedAlbums') ? `albums:${account}` : null,
    fetchCatalog,
  );
  const shows = usePaged(
    may('SavedShowsEpisodes') ? `shows:${account}` : null,
    fetchCatalog,
  );
  const devices = useDevices(may('DevicePicker'));

  if (!signedIn)
    return (
      <>
        <section className="collection-heading">
          <p className="eyebrow">Your listening space</p>
          <h1>Home</h1>
          <p className="intro">
            What’s playing, what you played recently and your library, from
            Spotify.
          </p>
        </section>
        <div className="content-note">
          <span className="note-mark">i</span>
          <p>
            Sign in with Spotify to see your Home.{' '}
            <button className="link-button" onClick={onOpenSettings}>
              Open Settings
            </button>
          </p>
        </div>
      </>
    );

  const live = playback !== null && isLive(playback);
  const now = live ? playback.nowPlaying : null;
  const catalogRows = (items: CatalogItem[], noun: string): Row[] =>
    items.slice(0, HOME_ROWS).map((item) => ({
      key: item.uri,
      title: item.name || `Untitled ${noun}`,
      subtitle: item.by.join(', '),
      detail: noun === 'album' ? item.releaseDate.slice(0, 4) : '',
      extra:
        item.total === null
          ? ''
          : countLabel(item.total, noun === 'album' ? 'track' : 'episode'),
      onSelect: () => onOpenItem(item),
    }));

  return (
    <>
      <section className="collection-heading">
        <p className="eyebrow">Your listening space</p>
        <h1>Home</h1>
        <p className="intro">
          What’s playing, what you played recently and your library. Select a
          title to play or open it.
        </p>
        <div className="collection-meta">
          <span>From Spotify</span>
          <span>{deviceName(playback)}</span>
        </div>
      </section>

      <Section
        title="Now playing"
        seeAll={
          may('Queue')
            ? { label: 'Queue', onClick: () => onOpen({ kind: 'queue' }) }
            : null
        }
      >
        {now ? (
          <HomeTable
            label="Playing now"
            rows={[
              {
                key: 'now',
                title: now.name || 'Unknown title',
                subtitle: now.artists.join(', '),
                detail: now.album,
                extra: deviceName(playback),
                playing: true,
              },
            ]}
          />
        ) : (
          <Status>
            Nothing is playing. Choose a device under Playback device in the
            player, then play something.
          </Status>
        )}
      </Section>

      <Section
        title="Recently played"
        seeAll={
          recent.items.length > HOME_ROWS
            ? {
                label: `See all ${countLabel(recent.total)}`,
                onClick: () => onOpen({ kind: 'recent' }),
              }
            : null
        }
      >
        {may('RecentlyPlayed') ? (
          (listState(recent, 'Nothing played yet.') ?? (
            <HomeTable
              label="Recently played tracks. Select a title to play it."
              rows={recent.items.slice(0, HOME_ROWS).map((row) => ({
                key: String(row.position),
                title: row.name || 'Unknown title',
                subtitle: row.artists.join(', '),
                detail: row.album,
                extra: trackTime(row.durationMs),
                playing:
                  row.spotifyUrl !== null && row.spotifyUrl === playingUrl,
                onSelect: row.uri
                  ? () => onPlayRecent(recent.items, row)
                  : undefined,
                onMenu: row.uri
                  ? (event) => {
                      event.preventDefault();
                      onMenu(row, menuPoint(event));
                    }
                  : undefined,
              }))}
            />
          ))
        ) : (
          <AllowLink
            text="Melora needs your permission to show what you played recently."
            feature="RecentlyPlayed"
            onAllow={onAllow}
          />
        )}
      </Section>

      <Section title="Your playlists">
        {playlists ? (
          (listState(playlists, 'No playlists yet.') ?? (
            <HomeTable
              label="Your playlists. Select one to open it."
              rows={playlists.items.slice(0, HOME_ROWS).map((playlist) => ({
                key: playlist.id,
                title: playlist.name || 'Untitled playlist',
                subtitle: playlist.owner,
                detail: '',
                extra: countLabel(playlist.total),
                onSelect: () => onOpen(playlistSource(playlist)),
              }))}
            />
          ))
        ) : (
          <AllowLink
            text="Melora needs your permission to show your playlists."
            feature="Playlists"
            onAllow={onAllow}
          />
        )}
      </Section>

      {may('SavedAlbums') && (
        <Section
          title="Saved albums"
          seeAll={
            albums.total > HOME_ROWS
              ? {
                  label: `See all ${countLabel(albums.total, 'album')}`,
                  onClick: () => onOpen({ kind: 'albums' }),
                }
              : null
          }
        >
          {listState(albums, 'No saved albums yet.') ?? (
            <HomeTable
              label="Saved albums. Select one to open it."
              rows={catalogRows(albums.items, 'album')}
            />
          )}
        </Section>
      )}

      {may('SavedShowsEpisodes') && (
        <Section
          title="Podcasts"
          seeAll={
            shows.total > HOME_ROWS
              ? {
                  label: `See all ${countLabel(shows.total, 'podcast')}`,
                  onClick: () => onOpen({ kind: 'shows' }),
                }
              : null
          }
        >
          {listState(shows, 'No podcasts followed yet.') ?? (
            <HomeTable
              label="Podcasts you follow. Select one to open it."
              rows={catalogRows(shows.items, 'podcast')}
            />
          )}
        </Section>
      )}

      <Section title="Devices">
        {!may('DevicePicker') ? (
          <AllowLink
            text="Melora needs your permission to list your Spotify devices."
            feature="DevicePicker"
            onAllow={onAllow}
          />
        ) : devices.error && devices.devices === null ? (
          <p className="error-message" role="alert">
            {describeFailure(devices.error)}
          </p>
        ) : devices.devices === null ? (
          <Status>Loading from Spotify…</Status>
        ) : devices.devices.length === 0 ? (
          <Status>
            No Spotify devices are open. Open Spotify on a device, or turn on
            Playback in Melora in Settings.
          </Status>
        ) : (
          <>
            <HomeTable
              label="Your Spotify devices"
              rows={devices.devices.map((device) => ({
                key: device.key,
                title: device.thisApp
                  ? 'Melora (this app)'
                  : device.name || 'Spotify device',
                subtitle: deviceKindLabel[device.kind],
                detail: device.restricted ? 'Can’t be controlled' : '',
                extra: device.active ? 'Active' : '',
                playing: device.active,
              }))}
            />
            <p className="empty-state">
              Choose where to play under Playback device in the player.
            </p>
          </>
        )}
      </Section>
    </>
  );
}

function playlistSource(playlist: PlaylistSummary): ListSource {
  return { kind: 'playlist', playlist };
}
