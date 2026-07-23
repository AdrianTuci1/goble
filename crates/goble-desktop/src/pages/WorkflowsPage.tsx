import { useState } from 'react';
import { scheduleAgent } from '../tauri/api';

interface ScheduledTask {
  id: string;
  title: string;
  agentId: string;
  trigger: string;
  status: 'active' | 'paused' | 'failed';
}

export default function WorkflowsPage() {
  const [tasks, setTasks] = useState<ScheduledTask[]>([
    {
      id: '1',
      title: 'Daily Backup Verification',
      agentId: 'backup-agent',
      trigger: '0 2 * * *',
      status: 'active',
    },
  ]);
  const [newTitle, setNewTitle] = useState('');
  const [newAgent, setNewAgent] = useState('');
  const [newTrigger, setNewTrigger] = useState('');

  const addTask = async () => {
    if (!newTitle || !newAgent || !newTrigger) return;
    await scheduleAgent(newAgent, newTrigger);
    setTasks((t) => [
      ...t,
      {
        id: `${Date.now()}`,
        title: newTitle,
        agentId: newAgent,
        trigger: newTrigger,
        status: 'active',
      },
    ]);
    setNewTitle('');
    setNewAgent('');
    setNewTrigger('');
  };

  const toggleStatus = (id: string) => {
    setTasks((t) =>
      t.map((task) =>
        task.id === id
          ? { ...task, status: task.status === 'active' ? 'paused' : 'active' }
          : task
      )
    );
  };

  return (
    <div style={{ padding: 24, overflowY: 'auto', height: '100%' }}>
      <h1 style={{ fontSize: 24, fontWeight: 600, marginBottom: 24 }}>Workflows / Programări</h1>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
          gap: 12,
          marginBottom: 24,
        }}
      >
        <input
          type="text"
          placeholder="Titlu task"
          value={newTitle}
          onChange={(e) => setNewTitle(e.target.value)}
          style={inputStyle}
        />
        <input
          type="text"
          placeholder="Agent ID"
          value={newAgent}
          onChange={(e) => setNewAgent(e.target.value)}
          style={inputStyle}
        />
        <input
          type="text"
          placeholder="Trigger (cron / heartbeat)"
          value={newTrigger}
          onChange={(e) => setNewTrigger(e.target.value)}
          style={inputStyle}
        />
        <button
          onClick={addTask}
          style={{
            padding: '10px 16px',
            borderRadius: 8,
            border: 'none',
            background: '#e5e5e5',
            color: '#0a0a0a',
            fontWeight: 500,
            cursor: 'pointer',
          }}
        >
          Adaugă
        </button>
      </div>

      {tasks.map((task) => (
        <div
          key={task.id}
          style={{
            padding: 16,
            borderRadius: 12,
            background: '#111111',
            border: '1px solid #1f1f1f',
            marginBottom: 12,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <div>
            <div style={{ fontSize: 16, fontWeight: 500, marginBottom: 4 }}>{task.title}</div>
            <div style={{ fontSize: 13, color: '#a3a3a3' }}>
              Agent: {task.agentId} • Trigger: {task.trigger}
            </div>
          </div>
          <button
            onClick={() => toggleStatus(task.id)}
            style={{
              padding: '6px 12px',
              borderRadius: 6,
              border: 'none',
              background: task.status === 'active' ? '#22c55e' : '#737373',
              color: '#0a0a0a',
              fontSize: 13,
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {task.status === 'active' ? 'Activ' : 'Pauză'}
          </button>
        </div>
      ))}
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  padding: '10px 14px',
  background: '#111111',
  border: '1px solid #1f1f1f',
  borderRadius: 8,
  color: '#e5e5e5',
  fontSize: 14,
  outline: 'none',
};
