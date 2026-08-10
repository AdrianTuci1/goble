import { describe, it, expect } from 'vitest';
import { extractMentions } from '../tauri/api';
import { useStore } from '../stores/appStore';

describe('extractMentions', () => {
  it('extracts user and agent mentions', () => {
    expect(extractMentions('hey @user:u1 and @agent:a1 check this')).toEqual([
      'user:u1',
      'agent:a1',
    ]);
  });

  it('deduplicates repeated mentions', () => {
    expect(extractMentions('@agent:a1 @agent:a1')).toEqual(['agent:a1']);
  });

  it('returns empty array when no mentions', () => {
    expect(extractMentions('hello world')).toEqual([]);
  });
});

describe('thread store', () => {
  it('adds threads and messages', () => {
    const store = useStore.getState();
    store.setThreads([]);
    store.setActiveThreadId(null);

    const thread = {
      id: 't1',
      kind: 'channel' as const,
      title: 'general',
      owner_id: 'me',
      participants: [{ kind: 'user' as const, id: 'me' }],
      tags: [],
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
    store.addThread(thread);
    store.setActiveThreadId('t1');

    const msg = {
      id: 'm1',
      thread_id: 't1',
      author: { kind: 'user' as const, id: 'me' },
      content: 'hello',
      reply_to: null,
      tags: ['#todo'],
      participant_mentions: ['agent:agent-1'],
      reactions: [{ emoji: '🚀', participant_id: 'me' }],
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
    store.addThreadMessage('t1', msg);

    expect(useStore.getState().threads).toHaveLength(1);
    expect(useStore.getState().threadMessages['t1']).toHaveLength(1);
    expect(useStore.getState().threadMessages['t1'][0].tags).toContain('#todo');
  });

  it('adds participants locally without duplicates', () => {
    const store = useStore.getState();
    store.setThreadParticipants('t1', []);
    store.addThreadParticipantLocal('t1', { kind: 'agent', id: 'a1' });
    store.addThreadParticipantLocal('t1', { kind: 'agent', id: 'a1' });
    store.addThreadParticipantLocal('t1', { kind: 'user', id: 'u1' });

    expect(useStore.getState().threadParticipants['t1']).toHaveLength(2);
  });

  it('toggles pending tags', () => {
    const store = useStore.getState();
    store.setPendingTags([]);
    store.togglePendingTag('#todo');
    expect(useStore.getState().pendingTags).toContain('#todo');
    store.togglePendingTag('#todo');
    expect(useStore.getState().pendingTags).not.toContain('#todo');
  });
});
