import ChatWindow from './chat-window/ChatWindow';
import AgentsView from './agents-view/AgentsView';
import ConnectorsPage from '../../../pages/ConnectorsPage';
import { useMainViewStore } from '../store/mainViewStore';
import type { MainPage } from '../store/mainViewStore';
import './Content.css';

const pages: Record<MainPage, () => React.ReactNode> = {
  chat: ChatWindow,
  agents: AgentsView,
  connectors: ConnectorsPage,
  workflows: Placeholder,
  executions: Placeholder,
  knowledge: Placeholder,
  search: Placeholder,
  teams: Placeholder,
  vault: Placeholder,
};

function Placeholder() {
  return (
    <div className="content-placeholder">
      <h2>Coming soon</h2>
      <p>This page is under construction.</p>
    </div>
  );
}

export default function Content() {
  const { page } = useMainViewStore();
  const Page = pages[page] || ChatWindow;
  return <Page />;
}
