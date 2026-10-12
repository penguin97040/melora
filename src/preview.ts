export type PreviewTrack = {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration: string;
};

// Invented titles and artists, independent of any Spotify account or catalogue.
export const previewTracks: PreviewTrack[] = [
  {
    id: 1,
    title: 'Signal at dawn',
    artist: 'Glass Meridian',
    album: 'After the quiet',
    duration: '4:18',
  },
  {
    id: 2,
    title: 'Low orbit',
    artist: 'Glass Meridian',
    album: 'After the quiet',
    duration: '5:06',
  },
  {
    id: 3,
    title: 'A smaller sky',
    artist: 'Glass Meridian',
    album: 'After the quiet',
    duration: '3:42',
  },
  {
    id: 4,
    title: 'Between stations',
    artist: 'Glass Meridian',
    album: 'After the quiet',
    duration: '6:11',
  },
  {
    id: 5,
    title: 'Night geometry',
    artist: 'Static Gardens',
    album: 'Soft machinery',
    duration: '4:54',
  },
  {
    id: 6,
    title: 'Amber lines',
    artist: 'Static Gardens',
    album: 'Soft machinery',
    duration: '3:27',
  },
  {
    id: 7,
    title: 'Small hours',
    artist: 'Static Gardens',
    album: 'Soft machinery',
    duration: '5:32',
  },
  {
    id: 8,
    title: 'Paper satellites',
    artist: 'Static Gardens',
    album: 'Soft machinery',
    duration: '4:09',
  },
  {
    id: 9,
    title: 'Still water',
    artist: 'North Circuit',
    album: 'A distant room',
    duration: '4:46',
  },
  {
    id: 10,
    title: 'Window light',
    artist: 'North Circuit',
    album: 'A distant room',
    duration: '3:58',
  },
  {
    id: 11,
    title: 'Almost home',
    artist: 'North Circuit',
    album: 'A distant room',
    duration: '5:17',
  },
  {
    id: 12,
    title: 'The last frequency',
    artist: 'North Circuit',
    album: 'A distant room',
    duration: '6:03',
  },
];
export const previewPlaylists = [
  { name: 'All preview tracks', subtitle: '12 synthetic tracks', artist: '' },
  {
    name: 'After the quiet',
    subtitle: 'Glass Meridian · 4 tracks',
    artist: 'Glass Meridian',
  },
  {
    name: 'Soft machinery',
    subtitle: 'Static Gardens · 4 tracks',
    artist: 'Static Gardens',
  },
  {
    name: 'A distant room',
    subtitle: 'North Circuit · 4 tracks',
    artist: 'North Circuit',
  },
];
