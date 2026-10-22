import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SpotifyAccount } from './SpotifyAccount';
import {
  cancelSpotifySignIn,
  openInSpotify,
  setSpotifyClientId,
  signInWithSpotify,
  signOutOfSpotify,
} from '../native/client';
import type { SpotifyStatus } from '../native/client';

vi.mock('../native/client', () => ({
  hasNativeStorage: () => true,
  setSpotifyClientId: vi.fn(),
  signInWithSpotify: vi.fn(),
  cancelSpotifySignIn: vi.fn(),
  signOutOfSpotify: vi.fn(),
  openInSpotify: vi.fn(),
}));

function status(
  auth: SpotifyStatus['auth'],
  extra: Partial<SpotifyStatus> = {},
): SpotifyStatus {
  return {
    auth,
    clientIdFromEnvironment: false,
    clientIdHint: null,
    lastError: null,
    ...extra,
  };
}

const profile = {
  accountId: 'acct1',
  id: 'user1',
  displayName: 'Kiri',
  imageUrl: null,
  spotifyUrl: 'https://open.spotify.com/user/user1',
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe('Spotify account settings', () => {
  it('saves a Client ID and clears the field', async () => {
    vi.mocked(setSpotifyClientId).mockResolvedValue(
      status({ kind: 'signedOut' }),
    );
    render(<SpotifyAccount status={status({ kind: 'notConfigured' })} />);
    expect(screen.getByRole('status')).toHaveTextContent(
      'Add a Client ID to sign in.',
    );
    expect(
      screen.queryByRole('button', { name: 'Sign in with Spotify' }),
    ).toBeNull();
    const field = screen.getByLabelText('Spotify Client ID');
    await userEvent.type(field, '0123456789abcdef0123456789abcdef');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(setSpotifyClientId).toHaveBeenCalledWith(
      '0123456789abcdef0123456789abcdef',
    );
    await waitFor(() => expect(field).toHaveValue(''));
  });

  it('explains an invalid Client ID without clearing it', async () => {
    vi.mocked(setSpotifyClientId).mockRejectedValue({
      kind: 'invalidClientId',
    });
    render(<SpotifyAccount status={status({ kind: 'notConfigured' })} />);
    const field = screen.getByLabelText('Spotify Client ID');
    await userEvent.type(field, 'nope');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That doesn’t look like a Client ID.',
    );
    expect(field).toHaveValue('nope');
  });

  it('starts sign-in, and offers Cancel while waiting for the browser', async () => {
    vi.mocked(signInWithSpotify).mockReturnValue(new Promise(() => {}));
    const { rerender } = render(
      <SpotifyAccount
        status={status({ kind: 'signedOut' }, { clientIdHint: '0123…cdef' })}
      />,
    );
    expect(screen.getByLabelText('Spotify Client ID')).toHaveAttribute(
      'placeholder',
      'Saved: 0123…cdef',
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Sign in with Spotify' }),
    );
    expect(signInWithSpotify).toHaveBeenCalledOnce();
    rerender(<SpotifyAccount status={status({ kind: 'signingIn' })} />);
    await userEvent.click(
      screen.getByRole('button', { name: 'Cancel sign-in' }),
    );
    expect(cancelSpotifySignIn).toHaveBeenCalledOnce();
  });

  it('shows the signed-in account with Open in Spotify and Sign out', async () => {
    render(
      <SpotifyAccount
        status={status({ kind: 'signedIn', profile, sessionOnly: false })}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('Signed in as Kiri.');
    await userEvent.click(
      screen.getByRole('button', { name: 'Open in Spotify' }),
    );
    expect(openInSpotify).toHaveBeenCalledWith(
      'https://open.spotify.com/user/user1',
    );
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(signOutOfSpotify).toHaveBeenCalledOnce();
  });

  it('says plainly when the sign-in only lasts for this session', () => {
    render(
      <SpotifyAccount
        status={status(
          { kind: 'signedIn', profile, sessionOnly: true },
          { lastError: { kind: 'storageUnavailable' } },
        )}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent(
      'lasts until Melora closes',
    );
    expect(screen.getByRole('alert')).toHaveTextContent('credential store');
  });

  it('points a 403 after sign-in at the User Management allowlist', () => {
    render(
      <SpotifyAccount
        status={status(
          { kind: 'signedIn', profile: null, sessionOnly: false },
          { lastError: { kind: 'forbidden', hint: 'Allowlist' } },
        )}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('User Management');
  });

  it('asks for a new sign-in after expiry and stays quiet on cancel', () => {
    const { rerender } = render(
      <SpotifyAccount
        status={status(
          { kind: 'sessionExpired' },
          { lastError: { kind: 'sessionExpired' } },
        )}
      />,
    );
    expect(
      screen.getByRole('button', { name: 'Sign in with Spotify' }),
    ).toBeEnabled();
    expect(screen.getByRole('alert')).toHaveTextContent('sign in again');
    rerender(
      <SpotifyAccount
        status={status(
          { kind: 'signedOut' },
          { lastError: { kind: 'cancelled' } },
        )}
      />,
    );
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('hides the Client ID field when the environment sets it', () => {
    render(
      <SpotifyAccount
        status={status(
          { kind: 'signedOut' },
          { clientIdFromEnvironment: true, clientIdHint: '0123…cdef' },
        )}
      />,
    );
    expect(screen.queryByLabelText('Spotify Client ID')).toBeNull();
    expect(
      screen.getByText(/set by the development environment/),
    ).toHaveTextContent('0123…cdef');
  });
});
