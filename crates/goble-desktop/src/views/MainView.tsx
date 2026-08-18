import { Outlet } from 'react-router-dom';
import Sidebar from '../components/Sidebar';
import RightSidebar from '../components/RightSidebar';

interface MainViewProps {
  collapsed: boolean;
  onNewChat: () => void;
}

export default function MainView({ collapsed, onNewChat }: MainViewProps) {
  return (
    <div className="main-view-shell">
      <Sidebar collapsed={collapsed} onNewChat={onNewChat} />
      <main className="main-area">
        <Outlet />
      </main>
      <RightSidebar />
    </div>
  );
}
