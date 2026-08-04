import Chat from './chat/Chat';
import Composer from './composer/Composer';
import './ChatWindow.css';

export default function ChatWindow() {
  return (
    <div className="chat-window">
      <Chat />
      <Composer />
    </div>
  );
}
