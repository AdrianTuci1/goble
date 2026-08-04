import { useEffect, useState, useCallback } from 'react';
import { Pencil, X, Plus } from 'lucide-react';
import {
  useProviderStore,
  API_SCHEMAS,
  createEndpointId,
  createModelId,
  type Endpoint,
  type EndpointModel,
  type ApiSchema,
  setLlmSetting,
} from '../../../shared';
import './ProvidersSection.css';

interface DraftModel extends EndpointModel {}

interface DraftEndpoint {
  id: string;
  name: string;
  schema: ApiSchema;
  baseUrl: string;
  apiKey: string;
  models: DraftModel[];
}

function emptyDraft(): DraftEndpoint {
  return {
    id: createEndpointId(),
    name: '',
    schema: 'openai',
    baseUrl: '',
    apiKey: '',
    models: [{ id: createModelId(), name: '', alias: '' }],
  };
}

function endpointToDraft(endpoint: Endpoint): DraftEndpoint {
  return {
    id: endpoint.id,
    name: endpoint.name,
    schema: endpoint.schema,
    baseUrl: endpoint.baseUrl,
    apiKey: endpoint.apiKey,
    models: endpoint.models.map((m) => ({ ...m })),
  };
}

export default function ProvidersSection() {
  const { endpoints, addEndpoint, updateEndpoint, removeEndpoint } = useProviderStore();
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<DraftEndpoint>(emptyDraft());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (endpoints.length === 0) {
      setDraft(emptyDraft());
      setEditingId(null);
      setIsModalOpen(true);
    }
  }, [endpoints.length]);

  const openModal = useCallback((endpoint?: Endpoint) => {
    setDraft(endpoint ? endpointToDraft(endpoint) : emptyDraft());
    setEditingId(endpoint ? endpoint.id : null);
    setError(null);
    setIsModalOpen(true);
  }, []);

  const closeModal = useCallback(() => {
    setIsModalOpen(false);
    setEditingId(null);
    setError(null);
  }, []);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape' && isModalOpen) {
        closeModal();
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isModalOpen, closeModal]);

  function updateDraft(updates: Partial<DraftEndpoint>) {
    setDraft((prev) => ({ ...prev, ...updates }));
    setError(null);
  }

  function updateDraftModel(id: string, updates: Partial<DraftModel>) {
    setDraft((prev) => ({
      ...prev,
      models: prev.models.map((m) => (m.id === id ? { ...m, ...updates } : m)),
    }));
    setError(null);
  }

  function addDraftModel() {
    setDraft((prev) => ({
      ...prev,
      models: [...prev.models, { id: createModelId(), name: '', alias: '' }],
    }));
  }

  function removeDraftModel(id: string) {
    setDraft((prev) => ({
      ...prev,
      models: prev.models.filter((m) => m.id !== id),
    }));
  }

  async function syncToBackend(endpoint: Endpoint) {
    const firstModel = endpoint.models[0];
    if (!firstModel) return;
    try {
      await setLlmSetting(
        endpoint.schema,
        endpoint.apiKey,
        firstModel.name,
        endpoint.baseUrl || undefined,
        0.7,
      );
    } catch (err) {
      console.error('Failed to sync provider to backend', err);
    }
  }

  async function handleSave() {
    if (!draft.name.trim()) {
      setError('Endpoint name is required.');
      return;
    }
    if (draft.models.length === 0 || draft.models.some((m) => !m.name.trim())) {
      setError('At least one model with a name is required.');
      return;
    }
    const endpoint: Endpoint = {
      id: editingId || draft.id,
      name: draft.name.trim(),
      schema: draft.schema,
      baseUrl: draft.baseUrl.trim(),
      apiKey: draft.apiKey,
      models: draft.models.map((m) => ({
        id: m.id || createModelId(),
        name: m.name.trim(),
        alias: m.alias?.trim(),
      })),
    };
    if (editingId) {
      updateEndpoint(editingId, endpoint);
    } else {
      addEndpoint(endpoint);
    }
    await syncToBackend(endpoint);
    closeModal();
  }

  function handleDelete() {
    if (editingId) {
      removeEndpoint(editingId);
    }
    closeModal();
  }

  return (
    <div className="settings-page">
      <div className="providers-page-header">
        <div>
          <h2>Providers</h2>
          <p className="settings-page-sub">Manage the endpoints and models Goble can use.</p>
        </div>
        <button className="providers-add-btn" onClick={() => openModal()}>
          <Plus size={16} />
          Add model
        </button>
      </div>

      <div className="providers-list">
        {endpoints.length === 0 && !isModalOpen && (
          <div className="providers-empty">No providers configured yet.</div>
        )}
        {endpoints.map((endpoint) => (
          <div key={endpoint.id} className="provider-card">
            <div className="provider-card-header">
              <div className="provider-card-title">
                <span>{endpoint.name}</span>
                <span className="provider-schema">{API_SCHEMAS.find((s) => s.id === endpoint.schema)?.label}</span>
              </div>
              <button className="provider-edit-btn" onClick={() => openModal(endpoint)}>
                <Pencil size={14} />
                Edit
              </button>
            </div>
            <div className="provider-models">
              {endpoint.models.map((model) => (
                <span key={model.id} className="provider-model-chip">
                  {model.alias || model.name}
                </span>
              ))}
            </div>
          </div>
        ))}
      </div>

      {isModalOpen && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3>{editingId ? 'Edit endpoint' : 'Add custom endpoint'}</h3>
              <div className="modal-close-group">
                <button className="modal-close" onClick={closeModal} aria-label="Close">
                  <X size={18} />
                </button>
                <span className="modal-esc">ESC</span>
              </div>
            </div>

            <p className="modal-description">
              Provide your endpoint details below. You can add as many models from the endpoint as you&rsquo;d like and can also provide aliases for the model picker in your input.
            </p>

            {error && <div className="modal-error">{error}</div>}

            <div className="modal-body">
              <div className="modal-field">
                <label>API schema</label>
                <select value={draft.schema} onChange={(e) => updateDraft({ schema: e.target.value as ApiSchema })}>
                  {API_SCHEMAS.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.label}
                    </option>
                  ))}
                </select>
              </div>

              <div className="modal-field">
                <label>Endpoint name</label>
                <input
                  value={draft.name}
                  onChange={(e) => updateDraft({ name: e.target.value })}
                  placeholder="e.g., Zach's external models"
                />
              </div>

              <div className="modal-field">
                <label>Endpoint URL</label>
                <input
                  value={draft.baseUrl}
                  onChange={(e) => updateDraft({ baseUrl: e.target.value })}
                  placeholder="Please include 'https://'"
                />
              </div>

              <div className="modal-field">
                <label>API key</label>
                <input
                  type="password"
                  value={draft.apiKey}
                  onChange={(e) => updateDraft({ apiKey: e.target.value })}
                  placeholder="e.g., sk-..."
                />
              </div>

              <div className="modal-models">
                <label>Models</label>
                {draft.models.map((model) => (
                  <div key={model.id} className="modal-model-row">
                    <input
                      className="modal-model-input"
                      value={model.name}
                      onChange={(e) => updateDraftModel(model.id, { name: e.target.value })}
                      placeholder="e.g., GLM-5-FP8"
                    />
                    <input
                      className="modal-model-input"
                      value={model.alias || ''}
                      onChange={(e) => updateDraftModel(model.id, { alias: e.target.value })}
                      placeholder="e.g., GLM-5"
                    />
                    {draft.models.length > 1 && (
                      <button className="modal-remove-model" onClick={() => removeDraftModel(model.id)} aria-label="Remove model">
                        <X size={14} />
                      </button>
                    )}
                  </div>
                ))}
                <button className="modal-add-model" onClick={addDraftModel}>
                  <Plus size={14} />
                  Add model
                </button>
              </div>
            </div>

            <div className="modal-footer">
              {editingId && (
                <button className="modal-btn danger" onClick={handleDelete}>
                  Delete
                </button>
              )}
              <div className="modal-footer-actions">
                <button className="modal-btn secondary" onClick={closeModal}>
                  Cancel
                </button>
                <button className="modal-btn primary" onClick={handleSave}>
                  {editingId ? 'Save endpoint' : 'Add endpoint'}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
