import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { uid } from '../utils/designSystem';

export type ApiSchema = 'openai' | 'anthropic' | 'openrouter' | 'ollama' | 'deepseek';

export const API_SCHEMAS: { id: ApiSchema; label: string }[] = [
  { id: 'openai', label: 'OpenAI Chat Completions' },
  { id: 'anthropic', label: 'Anthropic Messages' },
  { id: 'openrouter', label: 'OpenRouter' },
  { id: 'ollama', label: 'Ollama' },
  { id: 'deepseek', label: 'DeepSeek' },
];

export interface EndpointModel {
  id: string;
  name: string;
  alias?: string;
}

export interface Endpoint {
  id: string;
  name: string;
  schema: ApiSchema;
  baseUrl: string;
  apiKey: string;
  models: EndpointModel[];
}

interface ProviderState {
  endpoints: Endpoint[];
  addEndpoint: (endpoint: Endpoint) => void;
  updateEndpoint: (id: string, updates: Partial<Endpoint>) => void;
  removeEndpoint: (id: string) => void;
}

export const useProviderStore = create<ProviderState>()(
  persist(
    (set) => ({
      endpoints: [],
      addEndpoint: (endpoint) =>
        set((s) => ({
          endpoints: [endpoint, ...s.endpoints],
        })),
      updateEndpoint: (id, updates) =>
        set((s) => ({
          endpoints: s.endpoints.map((e) => (e.id === id ? { ...e, ...updates } : e)),
        })),
      removeEndpoint: (id) =>
        set((s) => ({
          endpoints: s.endpoints.filter((e) => e.id !== id),
        })),
    }),
    {
      name: 'goble-providers',
    },
  ),
);

export function getFirstConfiguredModel(): { provider: ApiSchema; model: string; alias?: string; endpointName: string } | null {
  const { endpoints } = useProviderStore.getState();
  for (const endpoint of endpoints) {
    const first = endpoint.models[0];
    if (first) {
      return {
        provider: endpoint.schema,
        model: first.name,
        alias: first.alias,
        endpointName: endpoint.name,
      };
    }
  }
  return null;
}

export function createEndpointId(): string {
  return uid();
}

export function createModelId(): string {
  return uid();
}
