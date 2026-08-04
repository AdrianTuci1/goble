import WorkspaceRail from './workspace-rail/WorkspaceRail';
import ThreadsSidebar from './threads-sidebar/ThreadsSidebar';
import ThreadsContent from './threads-content/ThreadsContent';
import './ThreadsView.css';

export default function ThreadsView() {
  return (
    <div className="threads-view">
      <WorkspaceRail />
      <ThreadsSidebar />
      <ThreadsContent />
    </div>
  );
}
