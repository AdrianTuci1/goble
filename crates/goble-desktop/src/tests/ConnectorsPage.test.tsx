import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import * as tauriCore from '@tauri-apps/api/core';
import ConnectorsPage from '../pages/ConnectorsPage';
import { useStore } from '../stores/appStore';

describe('ConnectorsPage MCP drawer flow', () => {
  beforeEach(() => {
    useStore.setState({
      mcpServers: [],
      vaultSecrets: [
        { key: 'openai-api-key', updated_at: '2026-01-01T00:00:00Z' },
      ],
    } as never);
    vi.restoreAllMocks();
  });

  it('installs, discovers, disables a tool and saves meta', async () => {
    let listCalls = 0;
    const invoke = vi.spyOn(tauriCore, 'invoke').mockImplementation((cmd) => {
      switch (cmd) {
        case 'search_mcp_servers':
          return Promise.resolve([]);
        case 'install_mcp_server':
          return Promise.resolve('mcp-mock installed');
        case 'list_mcp_servers': {
          listCalls += 1;
          if (listCalls === 1) {
            return Promise.resolve([
              {
                id: 'mcp-mock',
                name: 'Mock MCP',
                source: 'local',
                source_value: '/tmp/mock',
                capabilities: ['tools'],
                auth_required: false,
                discovered_tools: [],
                secret_ids: [],
                enabled_tools: [],
              },
            ]);
          }
          return Promise.resolve([
            {
              id: 'mcp-mock',
              name: 'Mock MCP',
              source: 'local',
              source_value: '/tmp/mock',
              capabilities: ['tools'],
              auth_required: false,
              discovered_tools: ['mcp_mock_echo'],
              secret_ids: [],
              enabled_tools: ['mcp_mock_echo'],
            },
          ]);
        }
        case 'discover_mcp_tools':
          return Promise.resolve([
            { name: 'mcp_mock_echo', description: 'echo', parameters: {} },
          ]);
        case 'update_mcp_server_meta':
          return Promise.resolve('ok');
        default:
          return Promise.resolve(undefined);
      }
    });

    render(<ConnectorsPage />);

    fireEvent.change(screen.getByPlaceholderText('ID (e.g. mcp-postgres)'), {
      target: { value: 'mcp-mock' },
    });
    fireEvent.change(screen.getByPlaceholderText('Display name'), {
      target: { value: 'Mock MCP' },
    });
    fireEvent.change(screen.getByDisplayValue('npm'), {
      target: { value: 'local' },
    });
    fireEvent.change(screen.getByPlaceholderText('Package / owner/repo / path / url'), {
      target: { value: '/tmp/mock' },
    });

    const form = screen.getByRole('button', { name: /install/i }).closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      const installCall = invoke.mock.calls.find((c) => c[0] === 'install_mcp_server');
      expect(installCall).toBeDefined();
      expect(installCall?.[1]).toEqual({
        req: {
          id: 'mcp-mock',
          name: 'Mock MCP',
          source: 'local',
          source_value: '/tmp/mock',
        },
      });
    });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Mock MCP/ })).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: /Mock MCP/ }));

    await waitFor(() => {
      expect(screen.getByText(/Vault secrets/)).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: /^Discover$/i }));

    await waitFor(() => {
      expect(screen.getByLabelText('mcp_mock_echo')).toBeTruthy();
    });

    fireEvent.click(screen.getByLabelText('mcp_mock_echo'));

    fireEvent.click(screen.getByRole('button', { name: /save/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('update_mcp_server_meta', {
        req: {
          id: 'mcp-mock',
          secret_ids: [],
          enabled_tools: [],
        },
      });
    });
  });
});
