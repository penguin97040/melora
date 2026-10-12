import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import App from './App';
import { getAppInfo, savePreferences } from './native/client';

vi.mock('./native/client', () => ({
  defaultPreferences: { scale: 100, comfortable: false, volume: 70 },
  getAppInfo: vi.fn(),
  savePreferences: vi.fn(),
  hasNativeStorage: () => true,
}));

beforeEach(() => {
  vi.mocked(getAppInfo).mockResolvedValue({
    version: '0.1.0',
    platform: 'windows',
    preferences: { scale: 100, comfortable: false, volume: 70 },
    storageError: null,
  });
  vi.mocked(savePreferences).mockImplementation(async (value) => value);
});

describe('preview shell', () => {
  it('reports native IPC details and keeps unavailable playback disabled', async () => {
    render(<App />);
    expect(await screen.findByText('v0.1.0 / windows')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Play' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Next track' })).toBeDisabled();
    expect(screen.getByText('Synthetic preview')).toBeInTheDocument();
    expect(screen.getByText('Stream quality: unavailable')).toBeInTheDocument();
  });

  it('supports keyboard selection and playlist filtering', async () => {
    const user = userEvent.setup();
    render(<App />);
    const playlist = screen.getByRole('button', {
      name: /Soft machinery.*Static Gardens/,
    });
    playlist.focus();
    await user.keyboard('{Enter}');
    expect(
      screen.getByRole('heading', { name: 'Soft machinery' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /Signal at dawn.*Glass Meridian/ }),
    ).not.toBeInTheDocument();
    const track = screen.getByRole('button', {
      name: /Amber lines.*Static Gardens/,
    });
    track.focus();
    await user.keyboard('{Enter}');
    expect(
      screen.getByRole('heading', { name: 'Amber lines' }),
    ).toBeInTheDocument();
  });

  it('searches synthetic data and announces an empty result', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: /Search/ }));
    const search = screen.getByRole('searchbox');
    expect(search).toHaveFocus();
    await user.type(search, 'not in preview');
    expect(screen.getByText(/No preview tracks match/)).toBeInTheDocument();
  });

  it('restores display settings and reports failed persistence', async () => {
    const user = userEvent.setup();
    vi.mocked(getAppInfo).mockResolvedValue({
      version: '0.1.0',
      platform: 'windows',
      preferences: { scale: 125, comfortable: true, volume: 40 },
      storageError: null,
    });
    vi.mocked(savePreferences).mockRejectedValue(new Error('fixture failure'));
    render(<App />);
    await screen.findByText('v0.1.0 / windows');
    await user.click(screen.getByRole('button', { name: /Settings/ }));
    expect(screen.getByRole('combobox')).toHaveValue('125');
    expect(screen.getByRole('checkbox')).toBeChecked();
    await user.selectOptions(screen.getByRole('combobox'), '200');
    await waitFor(() =>
      expect(document.documentElement.style.fontSize).toBe('200%'),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Preferences cannot be saved',
    );
  });
});
