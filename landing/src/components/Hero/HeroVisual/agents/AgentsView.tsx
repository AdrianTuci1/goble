import { useEffect, useState } from 'react';
import './AgentsView.css';
import { ChevronDown, Search, CheckCircle2, Circle, GitBranch } from 'lucide-react';
import loadingAgent01 from '../../../../assets/svg/loading-agents-01.svg';
import loadingAgent02 from '../../../../assets/svg/loading-agents-02.svg';
import loadingAgent03 from '../../../../assets/svg/loading-agents-03.svg';
import loadingAgent04 from '../../../../assets/svg/loading-agents-04.svg';
import loadingAgent05 from '../../../../assets/svg/loading-agents-05.svg';
import loadingAgent06 from '../../../../assets/svg/loading-agents-06.svg';
import loadingAgent07 from '../../../../assets/svg/loading-agents-07.svg';
import loadingAgent08 from '../../../../assets/svg/loading-agents-08.svg';

const loadingAgentFrames = [
  loadingAgent01,
  loadingAgent02,
  loadingAgent03,
  loadingAgent04,
  loadingAgent05,
  loadingAgent06,
  loadingAgent07,
  loadingAgent08,
];

function LoadingAgentIcon() {
  const [frame, setFrame] = useState(0);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setFrame((current) => (current + 1) % loadingAgentFrames.length);
    }, 160);

    return () => window.clearInterval(timer);
  }, []);

  return <img src={loadingAgentFrames[frame]} alt="" />;
}

type Run = {
  id: string;
  title: string;
  timeLabel: string;
  source: string;
  credits: number;
  status: 'completed' | 'running' | 'idle';
  userInitials: string;
  environment: string;
};

const mockRuns: Run[] = [
  {
    id: '1',
    title: 'Refactor billing webhook retries',
    timeLabel: 'now',
    source: 'Interactive',
    credits: 1.2,
    status: 'running',
    userInitials: 'A',
    environment: 'octomus/main'
  },
  {
    id: '2',
    title: 'Add dashboard empty states',
    timeLabel: '12 min ago',
    source: 'Interactive',
    credits: 3.8,
    status: 'completed',
    userInitials: 'M',
    environment: 'web/app-shell'
  },
  {
    id: '3',
    title: 'Investigate flaky checkout spec',
    timeLabel: '38 min ago',
    source: 'Scheduled',
    credits: 2.4,
    status: 'completed',
    userInitials: 'R',
    environment: 'ci/playwright'
  },
  {
    id: '4',
    title: 'Generate migration for team roles',
    timeLabel: '1 hour ago',
    source: 'GitHub issue',
    credits: 5.1,
    status: 'completed',
    userInitials: 'N',
    environment: 'api/postgres'
  },
  {
    id: '5',
    title: 'Prepare release notes for v0.12',
    timeLabel: '2 hours ago',
    source: 'Slack',
    credits: 0.8,
    status: 'idle',
    userInitials: 'L',
    environment: 'docs'
  }
];

type AgentsViewProps = {
  activeMenu?: boolean;
  selectedRunId?: string;
};

export function AgentsView({ activeMenu = false, selectedRunId }: AgentsViewProps) {
  const [selectedId, setSelectedId] = useState<string | undefined>(selectedRunId);

  useEffect(() => {
    setSelectedId(selectedRunId);
  }, [selectedRunId]);

  return (
    <div className="agents-view" aria-hidden="true">
      <header className="agents-header">
        <div>
          <h1 className="agents-title">Runs</h1>
          <p className="agents-subtitle">Live autonomous work across repos and tasks</p>
        </div>
        <div className="agents-header-actions">
          <button className="agents-btn-secondary">Get started</button>
          <button className="agents-btn-primary">New agent</button>
        </div>
      </header>

      <div className="agents-filters-bar">
        <div className="agents-filters-group">
          <div className={`filter-item ${activeMenu ? 'filter-item-active' : ''}`}>
            <span>Status: All</span>
            <ChevronDown size={14} />
            {activeMenu && (
              <div className="agents-menu-popover">
                <button><CheckCircle2 size={13} /> Completed</button>
                <button><span className="agents-menu-loading-icon"><LoadingAgentIcon /></span> Running</button>
                <button><Circle size={13} /> Idle</button>
              </div>
            )}
          </div>
          <div className="filter-item">
            <span>Source: All</span>
            <ChevronDown size={14} />
          </div>
          <div className="filter-item">
            <span>Created on: All</span>
            <ChevronDown size={14} />
          </div>
          <div className="filter-item">
            <span>Environment: All</span>
            <ChevronDown size={14} />
          </div>
        </div>
        <div className="agents-search-wrapper">
          <Search size={14} className="search-icon" />
          <input type="text" placeholder="Search" />
        </div>
      </div>

      <div className="agents-runs-list">
        {mockRuns.map((run) => (
          <button
            key={run.id}
            type="button"
            className={`run-card ${selectedId === run.id ? 'run-card-selected' : ''}`}
            onClick={() => setSelectedId(run.id)}
          >
            <div className="run-card-icon">
              {run.status === 'completed' ? (
                <div className="status-icon-completed-wrapper">
                  <CheckCircle2 size={16} />
                </div>
              ) : run.status === 'running' ? (
                <div className="status-icon-running-wrapper">
                  <LoadingAgentIcon />
                </div>
              ) : (
                <Circle size={16} className="status-icon-idle" />
              )}
            </div>

            <div className="run-card-content">
              <div className="run-card-top">
                <span className="run-card-title">{run.title}</span>
                <div className="run-card-right">
                  <span className="run-card-time">{run.timeLabel}</span>
                  <div className="run-user-avatar">{run.userInitials}</div>
                </div>
              </div>
              <div className="run-card-metadata">
                <span>Source: {run.source}</span>
                <span><GitBranch size={12} /> {run.environment}</span>
                <span>Credits used: {run.credits} credits</span>
              </div>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
