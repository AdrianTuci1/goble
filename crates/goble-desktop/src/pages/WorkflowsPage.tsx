import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { createWorkflow, deleteWorkflow } from '../tauri/api';
import './Pages.css';

export default function WorkflowsPage() {
  const workflows = useStore((s) => s.workflows);
  const agents = useStore((s) => s.agents);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [trigger, setTrigger] = useState('0 * * * *');
  const [steps, setSteps] = useState<Array<{ name: string; agentId: string; input: string }>>([]);

  function addStep() {
    setSteps([...steps, { name: '', agentId: '', input: '' }]);
  }

  function updateStep(index: number, field: keyof typeof steps[0], value: string) {
    setSteps(steps.map((s, i) => (i === index ? { ...s, [field]: value } : s)));
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name || steps.length === 0) return;
    const workflowSteps = steps.map((s) => ({
      id: crypto.randomUUID(),
      name: s.name,
      agent_id: { 0: s.agentId },
      input_template: s.input,
      depends_on: [],
    }));
    await createWorkflow(name, description, workflowSteps, trigger);
    setName('');
    setDescription('');
    setTrigger('0 * * * *');
    setSteps([]);
  }

  return (
    <div className="page">
      <div className="page-header">
        <h2>Workflows</h2>
      </div>
      <div className="page-content">
        <form className="workflow-form" onSubmit={handleSubmit}>
          <input
            placeholder="Workflow name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <input
            placeholder="Description"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <input
            placeholder="Cron expression"
            value={trigger}
            onChange={(e) => setTrigger(e.target.value)}
          />
          <div className="workflow-steps">
            {steps.map((step, index) => (
              <div key={index} className="workflow-step">
                <input
                  placeholder="Step name"
                  value={step.name}
                  onChange={(e) => updateStep(index, 'name', e.target.value)}
                />
                <select value={step.agentId} onChange={(e) => updateStep(index, 'agentId', e.target.value)}>
                  <option value="">Select agent</option>
                  {agents.map((a) => (
                    <option key={a.id} value={a.id}>{a.name}</option>
                  ))}
                </select>
                <input
                  placeholder="Input template"
                  value={step.input}
                  onChange={(e) => updateStep(index, 'input', e.target.value)}
                />
              </div>
            ))}
          </div>
          <button type="button" onClick={addStep}>Add step</button>
          <button type="submit">Create workflow</button>
        </form>

        <div className="workflow-list">
          {workflows.map((w) => (
            <div key={w.id} className="card">
              <div className="card-title">{w.name}</div>
              <div className="card-row">{w.description}</div>
              <div className="card-row">Steps: {w.steps.length}</div>
              <div className="card-row">Enabled: {w.enabled ? 'yes' : 'no'}</div>
              <button onClick={() => deleteWorkflow(w.id)}>Delete</button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
