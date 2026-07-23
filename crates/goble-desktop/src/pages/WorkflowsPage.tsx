import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { scheduleAgent } from '../tauri/api';

interface ScheduledTask {
  id: string;
  title: string;
  agentId: string;
  trigger: string;
  status: 'active' | 'paused';
}

export default function WorkflowsPage() {
  const workers = useStore((s) => s.workers);
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);
  const [title, setTitle] = useState('');
  const [agentId, setAgentId] = useState('');
  const [trigger, setTrigger] = useState('0 * * * *');
  const [workerId, setWorkerId] = useState('');

  async function addTask() {
    if (!title || !agentId || !workerId) return;
    await scheduleAgent(workerId, agentId, trigger);
    const newTask: ScheduledTask = {
      id: `${Date.now()}`,
      title,
      agentId,
      trigger,
      status: 'active',
    };
    setTasks([...tasks, newTask]);
    setTitle('');
    setAgentId('');
  }

  return (
    <div className="page">
      <div className="page-header">
        <h2>Workflows</h2>
      </div>
      <div className="page-content">
        <div className="workflow-form">
          <input
            placeholder="Workflow title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
          <select value={workerId} onChange={(e) => setWorkerId(e.target.value)}>
            <option value="">Select paired worker</option>
            {workers.filter((w) => w.paired).map((w) => (
              <option key={w.id} value={w.id}>{w.name}</option>
            ))}
          </select>
          <input
            placeholder="Agent ID"
            value={agentId}
            onChange={(e) => setAgentId(e.target.value)}
          />
          <input
            placeholder="Cron expression"
            value={trigger}
            onChange={(e) => setTrigger(e.target.value)}
          />
          <button onClick={addTask}>Schedule</button>
        </div>
        <div className="workflow-list">
          {tasks.map((task) => (
            <div key={task.id} className="card">
              <div className="card-title">{task.title}</div>
              <div className="card-row">Agent: {task.agentId}</div>
              <div className="card-row">Trigger: {task.trigger}</div>
              <div className="card-row">Status: {task.status}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
