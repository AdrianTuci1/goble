import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import * as tauriCore from '@tauri-apps/api/core';
import GeneralSection from '../views/user-settings/settings-content/general/GeneralSection';
import WorkerGroupsSection from '../views/user-settings/settings-content/workers/WorkerGroupsSection';
import AboutSection from '../views/user-settings/settings-content/about/AboutSection';
import { useGeneralStore } from '../views/user-settings/store/generalStore';

describe('GeneralSection', () => {
  beforeEach(() => {
    useGeneralStore.setState({
      displayName: '',
      email: '',
      avatarSeed: 'honeycomb204',
    });
    vi.restoreAllMocks();
  });

  it('renders the generated name, avatar, and identity empty state', async () => {
    vi.spyOn(tauriCore, 'invoke').mockResolvedValue(null);

    render(<GeneralSection />);

    await waitFor(() => {
      expect(screen.getByText('honeycomb204')).toBeTruthy();
    });

    expect(screen.getByText(/No device identity configured yet/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: /Generate identity/i })).toBeTruthy();
  });

  it('renders identity details when an identity exists', async () => {
    const identity = {
      id: 'id-123',
      cluster_name: 'personal',
      cert_pem: '-----BEGIN CERTIFICATE-----\nMIIB',
      key_pem: '-----BEGIN PRIVATE KEY-----\nMIIE',
      ca_cert_pem: '-----BEGIN CERTIFICATE-----\nMIIC',
      role: 'Owner',
      is_owner: true,
      created_at: '2026-01-01T00:00:00Z',
    };

    vi.spyOn(tauriCore, 'invoke').mockImplementation((cmd) => {
      if (cmd === 'get_device_identity') return Promise.resolve(identity);
      return Promise.resolve(undefined);
    });

    render(<GeneralSection />);

    await waitFor(() => {
      expect(screen.getByText('personal')).toBeTruthy();
    });

    expect(screen.getByText('Owner')).toBeTruthy();
    expect(screen.getByRole('button', { name: /Download \.pem/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Show QR/i })).toBeTruthy();
  });

  it('updates the display name input', async () => {
    vi.spyOn(tauriCore, 'invoke').mockResolvedValue(null);

    render(<GeneralSection />);

    const input = screen.getByPlaceholderText('honeycomb204');
    fireEvent.change(input, { target: { value: 'Ada' } });

    await waitFor(() => {
      expect(screen.getByText('Ada')).toBeTruthy();
    });
  });
});

describe('WorkerGroupsSection', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('renders owner and member clusters', async () => {
    const clusters = [
      { id: 'owner-1', cluster_name: 'home-lab', role: 'Owner', is_owner: true },
      { id: 'member-1', cluster_name: 'team-alpha', role: 'Member', is_owner: false },
    ];

    vi.spyOn(tauriCore, 'invoke').mockImplementation((cmd) => {
      if (cmd === 'list_clusters') return Promise.resolve(clusters);
      return Promise.resolve(undefined);
    });

    render(<WorkerGroupsSection />);

    await waitFor(() => {
      expect(screen.getByText('home-lab')).toBeTruthy();
    });

    expect(screen.getByText('team-alpha')).toBeTruthy();
    expect(screen.getByRole('button', { name: /Export cluster key/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Leave/i })).toBeTruthy();
  });

  it('leaves a member cluster when the leave button is clicked', async () => {
    const clusters = [
      { id: 'member-1', cluster_name: 'team-alpha', role: 'Member', is_owner: false },
    ];

    const invoke = vi.spyOn(tauriCore, 'invoke').mockImplementation((cmd) => {
      if (cmd === 'list_clusters') return Promise.resolve(clusters);
      if (cmd === 'leave_cluster') return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(<WorkerGroupsSection />);

    await waitFor(() => {
      expect(screen.getByText('team-alpha')).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: /Leave/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('leave_cluster', { req: { id: 'member-1' } });
    });
  });

  it('opens Add group modal and creates a new cluster', async () => {
    vi.spyOn(tauriCore, 'invoke').mockImplementation((cmd) => {
      if (cmd === 'list_clusters') return Promise.resolve([]);
      if (cmd === 'generate_device_identity') return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(<WorkerGroupsSection />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add group/i })).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: /Add group/i }));

    await waitFor(() => {
      expect(screen.getByText('Add Worker Group')).toBeTruthy();
    });

    const input = screen.getByPlaceholderText('e.g., home-lab');
    fireEvent.change(input, { target: { value: 'test-cluster' } });

    fireEvent.click(screen.getByTestId('wg-modal-next'));

    await waitFor(() => {
      expect(screen.getByTestId('wg-modal-create')).toBeTruthy();
    });

    fireEvent.click(screen.getByTestId('wg-modal-create'));

    await waitFor(() => {
      expect(screen.queryByText('Add Worker Group')).toBeFalsy();
    });
  });

  it('opens Add group modal and joins a cluster with invite', async () => {
    vi.spyOn(tauriCore, 'invoke').mockImplementation((cmd) => {
      if (cmd === 'list_clusters') return Promise.resolve([]);
      if (cmd === 'join_cluster_with_invite') return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(<WorkerGroupsSection />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add group/i })).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: /Add group/i }));

    await waitFor(() => {
      expect(screen.getByText('Add Worker Group')).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Join with invite' }));

    const textarea = screen.getByPlaceholderText(/Paste the invite code or the full PEM bundle/i);
    fireEvent.change(textarea, { target: { value: 'pem-bundle' } });

    fireEvent.click(screen.getByTestId('wg-modal-join'));

    await waitFor(() => {
      expect(screen.queryByText('Add Worker Group')).toBeFalsy();
    });
  });
});

describe('AboutSection', () => {
  it('renders app branding and version placeholders', () => {
    render(<AboutSection />);
    expect(screen.getByText('Goble')).toBeTruthy();
    expect(screen.getByRole('heading', { name: /Open source/i })).toBeTruthy();
    expect(screen.getByRole('heading', { name: /Security & identity/i })).toBeTruthy();
  });
});
