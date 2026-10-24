import { computed, inject, Injectable, signal } from '@angular/core';

import { PREFERENCE_STORE } from '../../core/preferences';
import {
  Conversation,
  HISTORY_KEY,
  MAX_CONVERSATIONS,
  MAX_TURNS,
  newConversation,
  parseHistory,
  serialize,
  titleFrom,
  Turn,
} from './research-history';

/**
 * The research conversations: a list kept in memory and written to this machine's browser storage
 * (`PREFERENCE_STORE`) whenever something settles. Capped in count (conversations, turns, results),
 * stored without local paths, and never sent anywhere or written to the repository.
 */
@Injectable({ providedIn: 'root' })
export class ResearchHistoryService {
  private readonly store = inject(PREFERENCE_STORE);
  private readonly all = signal<readonly Conversation[]>(parseHistory(this.store.get(HISTORY_KEY)));
  /** Turns searched during this run: the backend still holds their results, so they can be added. */
  private readonly live = new Set<string>();

  /** Newest activity first. */
  readonly conversations = computed(() => [...this.all()].sort((a, b) => b.updatedAt - a.updatedAt));
  /** `null` is the empty "new research" screen. */
  readonly activeId = signal<string | null>(null);
  readonly active = computed(() => this.all().find((c) => c.id === this.activeId()) ?? null);

  /** Starts a conversation (kept only once it has a prompt) and makes it the active one. */
  startConversation(prompt: string): Conversation {
    const c: Conversation = { ...newConversation(), title: titleFrom(prompt) };
    this.all.update((list) => [c, ...list].slice(0, MAX_CONVERSATIONS));
    this.activeId.set(c.id);
    return c;
  }

  select(id: string | null): void {
    this.activeId.set(id);
  }

  rename(id: string, title: string): void {
    const t = titleFrom(title);
    if (t.length === 0) {
      return;
    }
    this.update(id, (c) => ({ ...c, title: t, titled: true }));
    this.save();
  }

  remove(id: string): void {
    this.all.update((list) => list.filter((c) => c.id !== id));
    if (this.activeId() === id) {
      this.activeId.set(null);
    }
    this.save();
  }

  clearAll(): void {
    this.all.set([]);
    this.activeId.set(null);
    this.live.clear();
    this.save();
  }

  addTurn(conversationId: string, turn: Turn): void {
    this.live.add(turn.id);
    this.update(conversationId, (c) => ({
      ...c,
      title: c.titled || c.turns.length > 0 ? c.title : titleFrom(turn.prompt),
      turns: [...c.turns, turn].slice(-MAX_TURNS),
    }));
    this.save();
  }

  /** Changes one turn; the storage is written once the turn is no longer running. */
  patchTurn(conversationId: string, turnId: string, patch: Partial<Turn>): void {
    this.update(conversationId, (c) => ({ ...c, turns: c.turns.map((t) => (t.id === turnId ? { ...t, ...patch } : t)) }), false);
    if (patch.status !== undefined && patch.status !== 'running') {
      this.save();
    }
  }

  removeTurn(conversationId: string, turnId: string): void {
    this.update(conversationId, (c) => ({ ...c, turns: c.turns.filter((t) => t.id !== turnId) }));
    this.save();
  }

  /** The backend still holds this turn's results, so its results can be downloaded. */
  isLive(turnId: string): boolean {
    return this.live.has(turnId);
  }

  /** `touch` moves the conversation up the list; passive updates (a search filling in, badges refreshing) do not. */
  private update(id: string, fn: (c: Conversation) => Conversation, touch = true): void {
    this.all.update((list) => list.map((c) => (c.id === id ? { ...fn(c), updatedAt: touch ? Date.now() : c.updatedAt } : c)));
  }

  private save(): void {
    this.store.set(HISTORY_KEY, serialize(this.all()));
  }
}
