import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SpotifyAccount } from './SpotifyAccount';
import { profileRetryable } from '../features/account';
import {
  cancelSpotifySignIn,
  loadSpotifyProfile,
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
  loadSpotifyProfile: vi.fn(),
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
    redirectUri: 'http://127.0.0.1:43821/callback',
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

  it('keeps the account and offers Cancel while a new permission waits', async () => {
    render(
      <SpotifyAccount
        status={status({
          kind: 'signedIn',
          profile,
          sessionOnly: false,
          consenting: true,
          granted: [],
        })}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent(
      'Waiting for you to approve the new permission in your browser…',
    );
    expect(screen.getByRole('button', { name: 'Sign out' })).toBeVisible();
    await userEvent.click(
      screen.getByRole('button', { name: 'Cancel sign-in' }),
    );
    expect(cancelSpotifySignIn).toHaveBeenCalled();
  });

  it('shows the signed-in account with Open in Spotify and Sign out', async () => {
    render(
      <SpotifyAccount
        status={status({
          kind: 'signedIn',
          profile,
          sessionOnly: false,
          consenting: false,
          granted: [],
        })}
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
          {
            kind: 'signedIn',
            profile,
            sessionOnly: true,
            granted: [],
            consenting: false,
          },
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
          {
            kind: 'signedIn',
            profile: null,
            sessionOnly: false,
            granted: [],
            consenting: false,
          },
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

  it('shows the exact redirect URI the native side uses', () => {
    render(
      <SpotifyAccount
        status={status(
          { kind: 'signedOut' },
          { redirectUri: 'http://127.0.0.1:50000/callback' },
        )}
      />,
    );
    expect(screen.getByText('http://127.0.0.1:50000/callback')).toBeVisible();
  });

  it('explains a busy sign-in port', () => {
    render(
      <SpotifyAccount
        status={status(
          { kind: 'signedOut' },
          { lastError: { kind: 'signIn', reason: 'CallbackPortBusy' } },
        )}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('sign-in port');
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

  it('offers to load a profile that failed to load', async () => {
    vi.mocked(loadSpotifyProfile).mockResolvedValue(profile);
    const offline = status(
      {
        kind: 'signedIn',
        profile: null,
        sessionOnly: false,
        granted: [],
        consenting: false,
      },
      { lastError: { kind: 'offline' } },
    );
    render(<SpotifyAccount status={offline} />);
    expect(screen.getByRole('status')).toHaveTextContent(
      'Signed in, but your Spotify profile couldn’t be loaded.',
    );
    expect(screen.getByRole('alert')).toHaveTextContent('can’t be reached');
    await userEvent.click(
      screen.getByRole('button', { name: 'Load profile again' }),
    );
    expect(loadSpotifyProfile).toHaveBeenCalledOnce();
  });

  it('shows a profile that is still loading without a retry', () => {
    render(
      <SpotifyAccount
        status={status(
          {
            kind: 'signedIn',
            profile: null,
            sessionOnly: true,
            granted: [],
            consenting: false,
          },
          { lastError: { kind: 'storageUnavailable' } },
        )}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent(
      'Signed in. Loading your profile…',
    );
    expect(
      screen.queryByRole('button', { name: 'Load profile again' }),
    ).toBeNull();
  });

  it('retries a missing profile by itself only after connection problems', () => {
    const missing = (lastError: SpotifyStatus['lastError']) =>
      status(
        {
          kind: 'signedIn',
          profile: null,
          sessionOnly: false,
          granted: [],
          consenting: false,
        },
        { lastError },
      );
    expect(profileRetryable(missing({ kind: 'offline' }))).toBe(true);
    expect(profileRetryable(missing({ kind: 'network' }))).toBe(true);
    expect(
      profileRetryable(missing({ kind: 'forbidden', hint: 'Allowlist' })),
    ).toBe(false);
    expect(
      profileRetryable(missing({ kind: 'rateLimited', retryAfterS: 5 })),
    ).toBe(false);
    expect(profileRetryable(missing(null))).toBe(false);
    expect(
      profileRetryable(
        status(
          {
            kind: 'signedIn',
            profile,
            sessionOnly: false,
            granted: [],
            consenting: false,
          },
          { lastError: { kind: 'offline' } },
        ),
      ),
    ).toBe(false);
  });
});
