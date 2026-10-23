import { useEffect, useRef, useState } from 'react';
import { DevicePicker } from './components/DevicePicker';
import { PlaybackTest } from './components/PlaybackTest';
import { Player } from './components/Player';
import { RowMenu } from './components/RowMenu';
import type { MenuItem, MenuPoint } from './features/menu';
import {
  editable,
  editFailure,
  moveIndex,
  moveRows,
  orderPlaylists,
  removeRows,
  savedOrder,
  usePlaylistOrder,
} from './features/editing';
import { SpotifyAccount } from './components/SpotifyAccount';
import { SpotifyHome } from './components/SpotifyHome';
import { SpotifyList } from './components/SpotifyList';
import { SpotifyPlaylists } from './components/SpotifyPlaylists';
import { SpotifyCatalog } from './components/SpotifyCatalog';
import { SpotifyQueue } from './components/SpotifyQueue';
import { SpotifySearch } from './components/SpotifySearch';
import { WindowHeader } from './components/WindowHeader';
import { accountName, profileRetryable } from './features/account';
import {
  fetchCatalog,
  fetchPlaylists,
  fetchTracks,
  granted,
  isCatalog,
  isTrackList,
  playRequestFor,
  sourceKey,
  usePaged,
  useQueue,
  useSavedStates,
} from './features/library';
import type { ListSource, Paged } from './features/library';
import {
  deviceControlAllowed,
  deviceName,
  isLive,
  isRemote,
  libraryAllowed,
  likeable,
  playbackAllowed,
} from './features/playback';
import {
  fetchSearchPage,
  searchKey,
  searchPlayRequest,
  useSearch,
} from './features/search';
import {
  activatePlayback,
  addToPlaylist,
  addToQueue,
  allowFeature,
  allowLibrary,
  createPlaylist,
  defaultPreferences,
  enablePlayback,
  getAppInfo,
  getWindowChrome,
  hasNativeStorage,
  loadSpotifyProfile,
  moveInPlaylist,
  openInSpotify,
  playFromList,
  refreshLiked,
  refreshPlayback,
  removeFromPlaylist,
  savePreferences,
  sendPlaybackCommand,
  setSaved,
  showMainWindow,
  subscribePlayback,
  subscribeSpotify,
  toggleLiked,
} from './native/client';
import type {
  AppInfo,
  PlaybackStatus,
  PlayerCommand,
  PlayRequest,
  PlaylistSummary,
  Preferences,
  CatalogItem,
  SearchKind,
  SpotifyStatus,
  TrackRow,
  WindowChrome,
} from './native/client';
import type { Feature } from './native/bindings/Feature';
import { describeFailure } from './features/account';
import { previewPlaylists, previewTracks } from './preview';

function connectionLabel(status: SpotifyStatus | null): string {
  switch (status?.auth.kind) {
    case 'signedIn':
      return 'Signed in to Spotify';
    case 'signingIn':
      return 'Signing in to Spotify…';
    case 'sessionExpired':
      return 'Spotify sign-in expired';
    default:
      return 'Spotify not connected';
  }
}

type View = 'Home' | 'Library' | 'Search' | 'Settings';
const initialTrack = previewTracks[0]!;
/** While Melora isn't playing itself, the active device is read this often (DO_NEXT §4.5). */
export const REMOTE_REFRESH_MS = 30_000;
/** A remote track change shows this long after the track should have ended. */
const TRACK_END_GRACE_MS = 1_500;
/** How long "Added to Liked Songs" replaces the player's mode line. */
export const LIKE_MESSAGE_MS = 3_000;

/** Background reads: a failure here shows nothing and the next trigger tries again. */
function quietly(action: () => Promise<unknown>): void {
  void (async () => {
    try {
      await action();
    } catch {
      // Settings and the device list show errors when the user acts.
    }
  })();
}

export default function App() {
  const [view, setView] = useState<View>('Library');
  const [playlist, setPlaylist] = useState(0);
  // A Spotify list chosen in the sidebar; `null` shows the preview playlist.
  const [source, setSource] = useState<ListSource | null>(null);
  const [playMessage, setPlayMessage] = useState<string | null>(null);
  const [selected, setSelected] = useState(initialTrack);
  const [query, setQuery] = useState('');
  // The kind of Spotify search result "See all" shows, or `null` for the overview.
  const [seeAll, setSeeAll] = useState<SearchKind | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [preferences, setPreferences] = useState(defaultPreferences);
  const [storageMessage, setStorageMessage] = useState<string | null>(null);
  const [chrome, setChrome] = useState<WindowChrome>('preview');
  const [chromeSettled, setChromeSettled] = useState(false);
  const [windowMessage, setWindowMessage] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [preferencesSettled, setPreferencesSettled] = useState(false);
  const [spotify, setSpotify] = useState<SpotifyStatus | null>(null);
  const [playback, setPlayback] = useState<PlaybackStatus | null>(null);
  const [likeMessage, setLikeMessage] = useState<string | null>(null);
  // A row's open context menu; `id` starts a fresh menu when one replaces another.
  const [menu, setMenu] = useState<{
    id: number;
    name: string;
    at: MenuPoint;
    items: MenuItem[];
  } | null>(null);
  // The sidebar's New playlist form, with the song to put in first.
  const [creating, setCreating] = useState<{
    adding: { uri: string; name: string } | null;
    busy: boolean;
  } | null>(null);
  const menus = useRef(0);
  // Playlist changes take turns, so Spotify applies them in the order they were made.
  const editQueue = useRef<Promise<unknown>>(Promise.resolve());
  // The latest status, for changes that wait their turn.
  const latestSpotify = useRef(spotify);
  useEffect(() => {
    latestSpotify.current = spotify;
  }, [spotify]);
  // Keyed to the URL, so a new profile or account gets a fresh attempt.
  const [failedAvatar, setFailedAvatar] = useState<string | null>(null);
  const revealRequested = useRef(false);
  const launchEnabled = useRef(false);
  const saveQueue = useRef<Promise<unknown>>(Promise.resolve());
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let active = true;
    void getAppInfo()
      .then((result) => {
        if (!active) return;
        setInfo(result);
        setPreferences(result.preferences);
        setStorageMessage(result.storageError);
        setReady(true);
      })
      .catch(() => {
        if (active)
          setStorageMessage(
            'Native application details could not be loaded. Relaunch Melora to try again.',
          );
      })
      .finally(() => {
        if (active) setPreferencesSettled(true);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void getWindowChrome()
      .then((mode) => {
        if (active) setChrome(mode);
      })
      .catch(() => {
        if (active)
          setWindowMessage(
            'Window controls could not be loaded. Relaunch Melora to try again.',
          );
      })
      .finally(() => {
        if (active) setChromeSettled(true);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    // Status arrives over a channel; it never contains tokens. The subscription lives as long
    // as the page, so the native side stops sending when the page goes away.
    let active = true;
    void subscribeSpotify((status) => {
      if (active) setSpotify(status);
    }).catch(() => {
      if (active)
        setWindowMessage(
          'The Spotify connection status could not be loaded. Relaunch Melora to try again.',
        );
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void subscribePlayback((status) => {
      if (active) setPlayback(status);
    }).catch(() => {
      if (active)
        setWindowMessage(
          'The playback status could not be loaded. Relaunch Melora to try again.',
        );
    });
    return () => {
      active = false;
    };
  }, []);

  const retryProfile = profileRetryable(spotify);
  useEffect(() => {
    // After a connection problem, try the profile again when the connection or the user comes
    // back. These are events, not polling; a refusal or rate limit is never retried by itself.
    if (!retryProfile) return;
    const retry = () => void loadSpotifyProfile().catch(() => undefined);
    window.addEventListener('online', retry);
    window.addEventListener('focus', retry);
    return () => {
      window.removeEventListener('online', retry);
      window.removeEventListener('focus', retry);
    };
  }, [retryProfile]);

  useEffect(() => {
    document.documentElement.style.fontSize = `${preferences.scale}%`;
    return () => {
      document.documentElement.style.fontSize = '';
    };
  }, [preferences.scale]);

  useEffect(() => {
    // Effects run after the shell commits; the scale effect above has also run.
    // Hidden WebViews need no animation-frame wait to signal readiness.
    if (!preferencesSettled || !chromeSettled || revealRequested.current)
      return;
    revealRequested.current = true;
    void showMainWindow().catch(() => {
      setWindowMessage(
        'The application window could not be shown. Relaunch Melora to try again.',
      );
    });
  }, [preferencesSettled, chromeSettled]);

  useEffect(() => {
    if (view === 'Search') searchRef.current?.focus();
  }, [view]);

  const deviceReady = playback?.tier === 'ready' || playback?.tier === 'active';
  const hereActive = playback?.tier === 'active';
  const remoteActive = isRemote(playback);
  const canControl = deviceControlAllowed(spotify);
  const mayPlayHere = playbackAllowed(spotify);
  const tier = playback?.tier;

  useEffect(() => {
    // Once playback is allowed, Melora becomes a Spotify device at launch, so it's always in
    // the device lists. Problems show in Settings → Playback in Melora.
    if (!mayPlayHere || !preferencesSettled || tier !== 'off') return;
    if (launchEnabled.current) return;
    launchEnabled.current = true;
    quietly(() => enablePlayback(preferences.volume / 100));
  }, [mayPlayHere, preferencesSettled, tier, preferences.volume]);

  useEffect(() => {
    // The active device is read when control becomes possible, on focus, and at a low rate
    // while Melora is visible and not playing itself. Never per second.
    if (!canControl) return;
    const refresh = () => {
      if (document.visibilityState === 'visible') quietly(refreshPlayback);
    };
    refresh();
    window.addEventListener('focus', refresh);
    const timer = hereActive
      ? undefined
      : window.setInterval(refresh, REMOTE_REFRESH_MS);
    return () => {
      window.removeEventListener('focus', refresh);
      if (timer !== undefined) window.clearInterval(timer);
    };
  }, [canControl, hereActive]);

  const remotePlaying =
    remoteActive &&
    playback !== null &&
    !playback.paused &&
    playback.durationMs > 0;
  const positionMs = playback?.positionMs ?? 0;
  const durationMs = playback?.durationMs ?? 0;
  const updatedAtMs = playback?.updatedAtMs ?? 0;
  useEffect(() => {
    // Another device moves to its next track without telling Melora, so read it again just
    // after this one should have ended (and no more often than every 10 s past the end).
    if (!remotePlaying) return;
    const left = durationMs - positionMs - (Date.now() - updatedAtMs);
    const timer = window.setTimeout(
      () => quietly(refreshPlayback),
      left > 0 ? left + TRACK_END_GRACE_MS : 10_000,
    );
    return () => window.clearTimeout(timer);
  }, [remotePlaying, positionMs, durationMs, updatedAtMs]);

  const mayLike = libraryAllowed(spotify);
  const nowUrl = playback?.nowPlaying?.spotifyUrl ?? null;
  const likedKnown = playback?.nowPlaying?.liked != null;
  useEffect(() => {
    // Whether a track is liked is asked once per track, and again on focus in case it was
    // changed in Spotify meanwhile. Rust keeps the answer only while that track plays.
    if (!mayLike || !likeable(nowUrl)) return;
    if (!likedKnown) quietly(refreshLiked);
    const recheck = () => quietly(refreshLiked);
    window.addEventListener('focus', recheck);
    return () => window.removeEventListener('focus', recheck);
  }, [mayLike, nowUrl, likedKnown]);

  useEffect(() => {
    if (likeMessage === null) return;
    const timer = window.setTimeout(
      () => setLikeMessage(null),
      LIKE_MESSAGE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [likeMessage]);

  const profile =
    spotify?.auth.kind === 'signedIn' ? spotify.auth.profile : null;
  const showPlaylists = granted(spotify, 'Playlists');
  const playlists = usePaged(
    showPlaylists ? `playlists:${profile?.id ?? ''}` : null,
    fetchPlaylists,
  );
  // Melora's own order for the sidebar (and Home), kept per account beside Melora's data.
  const playlistOrder = usePlaylistOrder(
    showPlaylists ? (profile?.id ?? null) : null,
    setWindowMessage,
  );
  const ordered: Paged<PlaylistSummary> = {
    ...playlists,
    items: orderPlaylists(playlists.items, playlistOrder.order),
  };
  // Signing out, or a sign-in without the permission, goes back to the preview.
  const shown = showPlaylists ? source : null;
  const list = usePaged(
    shown &&
      isTrackList(shown) &&
      (shown.kind !== 'playlist' || shown.playlist.readable)
      ? sourceKey(shown)
      : null,
    fetchTracks,
  );
  // Read while the queue is open, again whenever the item playing changes.
  const queue = useQueue(
    view === 'Library' && shown?.kind === 'queue' ? (profile?.id ?? '') : null,
    nowUrl ?? '',
  );
  const catalog = usePaged(
    shown && isCatalog(shown) ? sourceKey(shown) : null,
    fetchCatalog,
  );
  // Whether the list's tracks (and the album or show itself) are saved, asked per list. Liked
  // Songs are saved by definition.
  const savedUris =
    shown && isTrackList(shown) && shown.kind !== 'liked'
      ? [
          ...(shown.kind === 'album' ? [shown.album.uri] : []),
          ...(shown.kind === 'show' ? [shown.show.uri] : []),
          ...list.items.flatMap((row) =>
            row.uri?.startsWith('spotify:track:') ? [row.uri] : [],
          ),
        ]
      : [];
  const saved = useSavedStates(
    shown && mayLike ? `${profile?.id ?? ''}:${sourceKey(shown)}` : null,
    savedUris,
  );
  // Signed in, Search searches Spotify; signed out, it filters the preview tracks.
  const searchesSpotify = granted(spotify, 'Search');
  const searched = useSearch(query, searchesSpotify);
  const wanted = query.trim();
  const more = usePaged(
    searchesSpotify && seeAll && wanted ? searchKey(seeAll, wanted) : null,
    fetchSearchPage,
  );

  function chooseList(next: ListSource | null): void {
    setSource(next);
    setPlayMessage(null);
    setView('Library');
  }

  /** Must run inside the click, so Melora's own player may start audio. */
  function playFrom(request: PlayRequest): void {
    activatePlayback();
    void playFromList(request).then(
      () => setPlayMessage(null),
      (error: unknown) => setPlayMessage(describeFailure(error)),
    );
  }

  function playRow(row: TrackRow): void {
    if (shown) playFrom(playRequestFor(shown, list.items, row));
  }

  function changeQuery(next: string): void {
    setQuery(next);
    setSeeAll(null);
    setPlayMessage(null);
  }

  /** Opens a saved album's or show's page (or a podcast found by search). */
  function openItem(item: CatalogItem): void {
    chooseList(
      item.uri.startsWith('spotify:show:')
        ? { kind: 'show', show: item }
        : { kind: 'album', album: item },
    );
  }

  function playResult(
    kind: SearchKind,
    items: CatalogItem[],
    item: CatalogItem,
  ): void {
    if (kind === 'show') openItem(item);
    else playFrom(searchPlayRequest(kind, items, item));
  }

  /** Signs in again with the permission `feature` needs, then loads the list again. */
  function allow(feature: Feature): void {
    void allowFeature(feature).then(
      () => {
        list.retry();
        catalog.retry();
        queue.refresh();
      },
      (error: unknown) => setWindowMessage(describeFailure(error)),
    );
  }

  /**
   * A row's or heading's +: saves the track (to Liked Songs), album or show, or removes it. The
   * first use asks Spotify for library access, then carries on.
   */
  function saveItem(uri: string, isSaved: boolean): void {
    void (async () => {
      try {
        if (!mayLike && !libraryAllowed(await allowLibrary())) return;
        await setSaved([uri], !isSaved);
        saved.remember([uri], !isSaved);
      } catch (error) {
        setWindowMessage(describeFailure(error));
      }
    })();
  }

  /**
   * "Add to queue" from a row's menu. Without the playback permission it asks for it first.
   * Spotify queues only on a device that's playing; otherwise the message says to choose one.
   */
  function queueItem(uri: string): void {
    void (async () => {
      try {
        if (
          !granted(spotify, 'AddToQueue') &&
          !granted(await allowFeature('AddToQueue'), 'AddToQueue')
        )
          return;
        await addToQueue(uri);
        setLikeMessage('Added to queue');
        queue.refresh();
      } catch (error) {
        setWindowMessage(describeFailure(error));
      }
    })();
  }

  function showMenu(name: string, at: MenuPoint, items: MenuItem[]): void {
    menus.current += 1;
    setMenu({ id: menus.current, name, at, items });
  }

  /**
   * A playlist change, after the ones before it. The first asks Spotify for permission to change
   * playlists. On failure the message says why and `undo` puts the list back.
   */
  function editPlaylists(change: () => Promise<void>, undo?: () => void): void {
    editQueue.current = editQueue.current.then(async () => {
      try {
        if (
          !granted(latestSpotify.current, 'PlaylistEdit') &&
          !granted(await allowFeature('PlaylistEdit'), 'PlaylistEdit')
        ) {
          undo?.();
          return;
        }
        await change();
      } catch (error) {
        setWindowMessage(editFailure(error));
        undo?.();
      }
    });
  }

  /** A playlist's count after `added` items went in (or out, when negative). */
  function countPlaylist(id: string, added: number): void {
    playlists.edit((page) => ({
      ...page,
      items: page.items.map((item) =>
        item.id === id
          ? { ...item, total: Math.max(0, item.total + added) }
          : item,
      ),
    }));
  }

  /** Moves the shown playlist's row at index `from` to index `to`, here and on Spotify. */
  function moveTrack(from: number, to: number): void {
    const a = list.items[from];
    const b = list.items[to];
    if (shown?.kind !== 'playlist' || !a || !b || from === to) return;
    const id = shown.playlist.id;
    list.edit((page) => ({
      ...page,
      items: moveRows(page.items, a.position, b.position),
    }));
    editPlaylists(
      () => moveInPlaylist(id, a.position, b.position),
      list.reload,
    );
  }

  /** Removes every copy of `uri` from the shown playlist. */
  function removeTrack(uri: string): void {
    if (shown?.kind !== 'playlist') return;
    const { id, name } = shown.playlist;
    const copies = list.items.filter((row) => row.uri === uri).length;
    list.edit((page) => removeRows(page, uri));
    countPlaylist(id, -copies);
    editPlaylists(async () => {
      await removeFromPlaylist(id, [uri]);
      setLikeMessage(`Removed from ${name || 'the playlist'}`);
    }, list.reload);
  }

  function addTo(target: PlaylistSummary, uri: string): void {
    const showing =
      shown?.kind === 'playlist' && shown.playlist.id === target.id;
    editPlaylists(async () => {
      await addToPlaylist(target.id, [uri]);
      countPlaylist(target.id, 1);
      setLikeMessage(`Added to ${target.name || 'the playlist'}`);
      if (showing) list.reload();
    });
  }

  /**
   * Creates a private playlist from the sidebar's form (with a song first, if it came from
   * "Add to playlist…"), puts it first and opens it.
   */
  function createNew(name: string): void {
    const adding = creating?.adding ?? null;
    const order = playlistOrder.order;
    setCreating((previous) => previous && { ...previous, busy: true });
    editPlaylists(
      async () => {
        const made = await createPlaylist(name);
        // Spotify lists a new playlist first, so later pages start one further on.
        playlists.edit((page) => ({
          items: [made, ...page.items.filter((item) => item.id !== made.id)],
          total: page.total + 1,
          nextOffset: page.nextOffset === null ? null : page.nextOffset + 1,
        }));
        if (order.length > 0) playlistOrder.save([made.id, ...order]);
        setCreating(null);
        if (adding) {
          await addToPlaylist(made.id, [adding.uri]);
          countPlaylist(made.id, 1);
        }
        setLikeMessage(
          adding ? `Added to ${made.name}` : `Created ${made.name}`,
        );
        chooseList({
          kind: 'playlist',
          playlist: adding ? { ...made, total: 1 } : made,
        });
      },
      () => setCreating((previous) => previous && { ...previous, busy: false }),
    );
  }

  /** Moves the sidebar's playlist at `from` to `to` in Melora's own order. */
  function movePlaylist(from: number, to: number): void {
    if (to < 0 || to >= ordered.items.length || from === to) return;
    playlistOrder.save(
      savedOrder(moveIndex(ordered.items, from, to), playlistOrder.order),
    );
  }

  /** A song's or episode's menu, in any list; a playlist's own rows can also move and go. */
  function openTrackMenu(
    uri: string,
    name: string,
    at: MenuPoint,
    index: number | null = null,
  ): void {
    const items: MenuItem[] = [
      { label: 'Add to queue', onSelect: () => queueItem(uri) },
    ];
    if (showPlaylists)
      items.push({
        label: 'Add to playlist…',
        opensMenu: true,
        onSelect: () => openPlaylistPicker(uri, name, at),
      });
    if (
      index !== null &&
      shown?.kind === 'playlist' &&
      editable(shown.playlist)
    )
      items.push(
        {
          label: 'Move up',
          disabled: index === 0,
          onSelect: () => moveTrack(index, index - 1),
        },
        {
          label: 'Move down',
          disabled: index === list.items.length - 1,
          onSelect: () => moveTrack(index, index + 1),
        },
        {
          label: 'Remove from this playlist',
          onSelect: () => removeTrack(uri),
        },
      );
    showMenu(name, at, items);
  }

  /** "Add to playlist…": a new playlist, or one the account can change. */
  function openPlaylistPicker(uri: string, name: string, at: MenuPoint): void {
    showMenu(name, at, [
      {
        label: 'New playlist…',
        onSelect: () => setCreating({ adding: { uri, name }, busy: false }),
      },
      ...ordered.items.filter(editable).map((target) => ({
        label: target.name || 'Untitled playlist',
        onSelect: () => addTo(target, uri),
      })),
    ]);
  }

  /** A sidebar playlist's menu: moving it in Melora's order, or going back to Spotify's. */
  function openPlaylistMenu(
    item: PlaylistSummary,
    index: number,
    at: MenuPoint,
  ): void {
    const items: MenuItem[] = [
      {
        label: 'Move up',
        disabled: index === 0,
        onSelect: () => movePlaylist(index, index - 1),
      },
      {
        label: 'Move down',
        disabled: index === ordered.items.length - 1,
        onSelect: () => movePlaylist(index, index + 1),
      },
    ];
    if (playlistOrder.order.length > 0)
      items.push({
        label: 'Use Spotify’s order',
        onSelect: () => playlistOrder.save([]),
      });
    showMenu(item.name || 'Untitled playlist', at, items);
  }

  /** The + control. The first use asks Spotify for library access, then carries on. */
  function toggleLike(): void {
    void (async () => {
      try {
        if (!mayLike && !libraryAllowed(await allowLibrary())) return;
        const liked = await toggleLiked();
        setLikeMessage(
          liked ? 'Added to Liked Songs' : 'Removed from Liked Songs',
        );
        // The same song's row + follows.
        const track = nowUrl?.match(
          /^https:\/\/open\.spotify\.com\/track\/(\w+)$/,
        );
        if (track) saved.remember([`spotify:track:${track[1]}`], liked);
      } catch (error) {
        setWindowMessage(describeFailure(error));
      }
    })();
  }

  function playerCommand(command: PlayerCommand): void {
    void sendPlaybackCommand(command).catch((error: unknown) =>
      setWindowMessage(describeFailure(error)),
    );
  }

  /** Melora's own volume: saved as the preference and sent to its player when it's on. */
  function setMeloraVolume(volume: number): void {
    updatePreferences({ ...preferences, volume });
    // While another device plays, commands go to it; Melora's player keeps the preference.
    if (deviceReady && !remoteActive)
      playerCommand({ op: 'setVolume', volume: volume / 100 });
  }

  function openSpotifyPage(url: string): void {
    void openInSpotify(url).catch(() =>
      setWindowMessage('Your browser could not be opened. Try again.'),
    );
  }

  function updatePreferences(next: Preferences): void {
    setPreferences(next);
    // Serialise writes so fast control changes cannot persist stale preferences.
    saveQueue.current = saveQueue.current
      .then(() => savePreferences(next))
      .then(() => {
        setStorageMessage(null);
      })
      .catch(() => {
        setStorageMessage(
          'Preferences cannot be saved. Check that the settings folder is writable. On Windows, move the extracted folder to a writable location.',
        );
      });
  }

  const activePlaylist = previewPlaylists[playlist]!;
  const searchQuery = query.trim().toLocaleLowerCase('en-NZ');
  const tracks = previewTracks.filter((track) =>
    view === 'Search'
      ? `${track.title} ${track.artist} ${track.album}`
          .toLocaleLowerCase('en-NZ')
          .includes(searchQuery)
      : !activePlaylist.artist || track.artist === activePlaylist.artist,
  );
  const artistCount = new Set(tracks.map((track) => track.artist)).size;
  const spotifyList = view === 'Library' && shown !== null;
  const spotifySearch = view === 'Search' && searchesSpotify;
  const spotifyHome = view === 'Home' && spotify?.auth.kind === 'signedIn';
  const fromSpotify = spotifyList || spotifySearch || spotifyHome;

  return (
    <div
      className={`app chrome-${chrome} ${preferences.comfortable ? 'comfortable' : ''}`}
    >
      <a className="skip-link" href="#content">
        Skip to content
      </a>
      <WindowHeader chrome={chrome} onError={setWindowMessage} />
      <div className="workspace">
        <aside className="sidebar panel">
          <nav aria-label="Main navigation">
            <div className="panel-label">
              <span>Explore</span>
            </div>
            {(['Home', 'Library', 'Search', 'Settings'] as const).map(
              (name, i) => (
                <button
                  key={name}
                  className={`nav-item ${view === name ? 'active' : ''}`}
                  aria-current={view === name ? 'page' : undefined}
                  onClick={() => {
                    setView(name);
                    setPlayMessage(null);
                  }}
                >
                  <span className="nav-index">0{i + 1}</span>
                  {name}
                  <span className="nav-arrow">›</span>
                </button>
              ),
            )}
            <p className="connection-note">
              <span
                className={`status-dot ${spotify?.auth.kind === 'signedIn' ? 'connected' : ''}`}
              />
              {spotify?.auth.kind === 'notConfigured' ? (
                <button
                  className="link-button"
                  onClick={() => setView('Settings')}
                >
                  Set up Spotify
                </button>
              ) : (
                connectionLabel(spotify)
              )}
            </p>
          </nav>
          {showPlaylists ? (
            <SpotifyPlaylists
              playlists={ordered}
              selected={shown ? sourceKey(shown) : null}
              onSelect={chooseList}
              ordered={playlistOrder.order.length > 0}
              onMove={movePlaylist}
              onUseSpotifyOrder={() => playlistOrder.save([])}
              onMenu={openPlaylistMenu}
              creating={
                creating && {
                  adding: creating.adding?.name ?? null,
                  busy: creating.busy,
                }
              }
              onStartCreate={() => setCreating({ adding: null, busy: false })}
              onCreate={createNew}
              onCancelCreate={() => setCreating(null)}
            />
          ) : (
            <section className="playlists" aria-label="Preview playlists">
              <div className="panel-label">
                <span>Playlists</span>
                <span>04</span>
              </div>
              <div className="playlist-items">
                {previewPlaylists.map((item, i) => (
                  <button
                    key={item.name}
                    className={`playlist-item ${playlist === i ? 'selected' : ''}`}
                    aria-pressed={playlist === i}
                    onClick={() => {
                      setPlaylist(i);
                      chooseList(null);
                    }}
                  >
                    <span className="playlist-icon">▤</span>
                    <span>
                      <span className="playlist-name">{item.name}</span>
                      <span className="playlist-subtitle">{item.subtitle}</span>
                    </span>
                  </button>
                ))}
              </div>
              {spotify?.auth.kind === 'signedIn' ? (
                <p className="sidebar-footnote">
                  A local design preview.
                  <br />
                  <button
                    className="link-button"
                    onClick={() => allow('Playlists')}
                  >
                    Show my Spotify playlists
                  </button>
                </p>
              ) : (
                <p className="sidebar-footnote">
                  A local design preview.
                  <br />
                  No account required.
                </p>
              )}
            </section>
          )}
        </aside>
        <main id="content" className="content panel" tabIndex={-1}>
          <div className="panel-label">
            <span>{view}</span>
            <span>{fromSpotify ? 'From Spotify' : 'Local / preview'}</span>
          </div>
          {windowMessage && (
            <p className="error-message" role="alert">
              {windowMessage}
            </p>
          )}
          {storageMessage && (
            <p className="error-message" role="alert">
              {storageMessage}
            </p>
          )}
          {fromSpotify && playMessage && (
            <p className="error-message" role="alert">
              {playMessage}
            </p>
          )}
          {view === 'Settings' ? (
            <section className="settings">
              <p className="eyebrow">Make room for the music</p>
              <h1>Display & preferences</h1>
              <p className="intro">Adjust the preview to suit your screen.</p>
              <label className="setting-row">
                <span>Interface scale</span>
                <select
                  value={preferences.scale}
                  disabled={!ready}
                  onChange={(e) =>
                    updatePreferences({
                      ...preferences,
                      scale: Number(e.target.value),
                    })
                  }
                >
                  {[100, 125, 150, 175, 200].map((scale) => (
                    <option key={scale} value={scale}>
                      {scale}%
                    </option>
                  ))}
                </select>
              </label>
              <label className="setting-row">
                <span>Comfortable track rows</span>
                <input
                  type="checkbox"
                  checked={preferences.comfortable}
                  disabled={!ready}
                  onChange={(e) =>
                    updatePreferences({
                      ...preferences,
                      comfortable: e.target.checked,
                    })
                  }
                />
              </label>
              <label className="setting-row" htmlFor="volume">
                <span>
                  Volume <small>(Melora’s playback volume)</small>
                </span>
                <span>{preferences.volume}%</span>
              </label>
              <input
                id="volume"
                type="range"
                min="0"
                max="100"
                value={preferences.volume}
                disabled={!ready}
                onChange={(e) => setMeloraVolume(Number(e.target.value))}
              />
              <p className="settings-note">
                {hasNativeStorage()
                  ? 'Preferences are saved on this device. Windows portable builds keep them in the data folder beside Melora.exe.'
                  : 'Browser preview: preferences last for this session. The desktop app saves them on this device.'}
              </p>
              <SpotifyAccount status={spotify} />
              {playback && (
                <PlaybackTest
                  spotify={spotify}
                  playback={playback}
                  volume={preferences.volume}
                />
              )}
            </section>
          ) : view === 'Home' ? (
            <SpotifyHome
              spotify={spotify}
              playback={playback}
              playlists={showPlaylists ? ordered : null}
              playingUrl={nowUrl}
              onOpen={chooseList}
              onOpenItem={openItem}
              onPlayRecent={(rows, row) =>
                playFrom(playRequestFor({ kind: 'recent' }, rows, row))
              }
              onMenu={(row, at) =>
                row.uri && openTrackMenu(row.uri, row.name, at)
              }
              onAllow={allow}
              onOpenSettings={() => setView('Settings')}
            />
          ) : spotifySearch ? (
            <SpotifySearch
              query={query}
              onQuery={changeQuery}
              inputRef={searchRef}
              searched={searched}
              seeAll={seeAll}
              onSeeAll={setSeeAll}
              more={more}
              playingUrl={nowUrl}
              onPlay={playResult}
              onMenu={(item, at) => openTrackMenu(item.uri, item.name, at)}
            />
          ) : spotifyList && shown?.kind === 'queue' ? (
            <SpotifyQueue
              queue={queue}
              device={
                playback && isLive(playback) ? deviceName(playback) : null
              }
              onAllow={allow}
            />
          ) : spotifyList && shown && isCatalog(shown) ? (
            <SpotifyCatalog
              source={shown}
              list={catalog}
              onOpenItem={openItem}
              onAllow={allow}
            />
          ) : spotifyList && shown && isTrackList(shown) ? (
            <SpotifyList
              source={shown}
              list={list}
              playingUrl={nowUrl}
              saved={saved.known}
              onSave={saveItem}
              onMenu={(row, at) =>
                row.uri &&
                openTrackMenu(row.uri, row.name, at, list.items.indexOf(row))
              }
              editable={shown.kind === 'playlist' && editable(shown.playlist)}
              onMove={moveTrack}
              onPlay={playRow}
              onPlayAll={() =>
                shown.kind === 'playlist' &&
                playFrom({
                  kind: 'context',
                  uri: `spotify:playlist:${shown.playlist.id}`,
                  position: null,
                })
              }
              onOpen={openSpotifyPage}
              onAllow={allow}
            />
          ) : (
            <>
              <section className="collection-heading">
                <p className="eyebrow">
                  {view === 'Search'
                    ? 'Find a preview track'
                    : 'Your listening space'}
                </p>
                <h1>{view === 'Search' ? 'Search' : activePlaylist.name}</h1>
                <p className="intro">
                  {view === 'Search'
                    ? 'Search the synthetic titles, artists and albums below.'
                    : 'Invented music. A real space to make it your own.'}
                </p>
                <div className="collection-meta">
                  <span>{tracks.length.toLocaleString('en-NZ')} tracks</span>
                  <span>
                    {artistCount.toLocaleString('en-NZ')} synthetic{' '}
                    {artistCount === 1 ? 'artist' : 'artists'}
                  </span>
                  <span>No audio</span>
                </div>
              </section>
              {view === 'Search' && (
                <div className="search-field">
                  <label htmlFor="search">Search preview tracks</label>
                  <input
                    ref={searchRef}
                    id="search"
                    type="search"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder="Track, artist or album…"
                  />
                </div>
              )}
              <div className="track-table-wrapper">
                <table className="track-table">
                  <caption className="sr-only">
                    Synthetic preview tracks. Select a title to inspect its
                    details.
                  </caption>
                  <thead>
                    <tr>
                      <th scope="col">#</th>
                      <th scope="col">Track / artist</th>
                      <th scope="col">Album</th>
                      <th scope="col">Time</th>
                    </tr>
                  </thead>
                  <tbody>
                    {tracks.map((track) => (
                      <tr
                        key={track.id}
                        className={
                          selected.id === track.id ? 'selected-track' : ''
                        }
                      >
                        <td className="track-number">
                          {String(track.id).padStart(2, '0')}
                        </td>
                        <td>
                          <button
                            className="track-select"
                            aria-pressed={selected.id === track.id}
                            onClick={() => setSelected(track)}
                          >
                            <span>{track.title}</span>
                            <span className="track-artist">{track.artist}</span>
                          </button>
                        </td>
                        <td className="album-cell">{track.album}</td>
                        <td className="duration">{track.duration}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {tracks.length === 0 && (
                <p className="empty-state" role="status">
                  No preview tracks match “{query}”. Try another title or
                  artist.
                </p>
              )}
              <div className="content-note">
                <span className="note-mark">i</span>
                <p>
                  This is a synthetic preview. Spotify sign-in, your library and
                  playback are coming in later milestones.
                </p>
              </div>
            </>
          )}
        </main>
        <Player
          track={selected}
          playback={playback}
          devicePicker={
            <DevicePicker
              spotify={spotify}
              playback={playback}
              volume={preferences.volume}
              onVolume={setMeloraVolume}
              onCommand={playerCommand}
              onOpenSettings={() => setView('Settings')}
            />
          }
          likeMessage={likeMessage}
          onCommand={playerCommand}
          onToggleLike={toggleLike}
          onOpenInSpotify={openSpotifyPage}
        />
      </div>
      <footer className="status-strip">
        <span>
          <span className="status-dot" />
          {fromSpotify ? 'From Spotify' : 'Synthetic preview'}
        </span>
        {profile && (
          <span className="account-chip">
            {profile.imageUrl && profile.imageUrl !== failedAvatar && (
              <img
                className="avatar"
                src={profile.imageUrl}
                alt=""
                width={16}
                height={16}
                loading="lazy"
                referrerPolicy="no-referrer"
                onError={() => setFailedAvatar(profile.imageUrl)}
              />
            )}
            <span>Spotify: {accountName(spotify)}</span>
            {profile.spotifyUrl && (
              <button
                className="link-button"
                onClick={() => {
                  const url = profile.spotifyUrl;
                  if (url)
                    void openInSpotify(url).catch(() =>
                      setWindowMessage(
                        'Your browser could not be opened. Try again.',
                      ),
                    );
                }}
              >
                Open in Spotify
              </button>
            )}
          </span>
        )}
        <span className="playback-device">
          Playback device:{' '}
          <span>
            {hereActive || playback?.device?.thisApp
              ? 'This app'
              : remoteActive
                ? playback?.device?.name || 'Spotify device'
                : 'No device connected'}
          </span>
        </span>
        <span className="stream-quality">Stream quality: unavailable</span>
        <span role="status">
          {info
            ? `v${info.version} / ${info.platform}`
            : 'Native details pending'}
        </span>
      </footer>
      {menu && (
        <RowMenu
          key={menu.id}
          name={menu.name}
          at={menu.at}
          items={menu.items}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}
