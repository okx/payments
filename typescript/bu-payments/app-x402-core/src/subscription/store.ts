import type { Subscription } from "./types";

/**
 * Persistence abstraction for subscription state.
 *
 * Intentionally minimal — every state transition is performed by `put`-ing
 * the full Subscription object, and every lookup is by `subId`. Address /
 * payer-indexed queries are deliberately NOT part of this contract: the
 * seller's own data model should own that (e.g. by mapping wallet →
 * subId(s) in its user table), keeping the SDK store free of any secondary
 * index requirement.
 */
export interface SubscriptionStore {
  get(subId: string): Promise<Subscription | null>;
  put(sub: Subscription): Promise<void>;
  delete(subId: string): Promise<void>;
}

/**
 * In-memory reference implementation. Suitable for development / unit tests /
 * single-process demos only — multi-process deployments must replace this
 * with a shared persistent backend.
 */
export class InMemoryStore implements SubscriptionStore {
  private readonly data = new Map<string, Subscription>();

  async get(subId: string): Promise<Subscription | null> {
    const sub = this.data.get(subId);
    return sub ? { ...sub } : null;
  }

  async put(sub: Subscription): Promise<void> {
    this.data.set(sub.subId, { ...sub });
  }

  async delete(subId: string): Promise<void> {
    this.data.delete(subId);
  }

  /**
   * Return all subscriptions, ordered by `startAt` ascending. Not part of
   * the SubscriptionStore interface — admin/debug helper, not used by the
   * scheme. Production backends should expose paginated equivalents.
   */
  async list(): Promise<Subscription[]> {
    return Array.from(this.data.values())
      .map(s => ({ ...s }))
      .sort((a, b) => a.startAt - b.startAt);
  }
}
