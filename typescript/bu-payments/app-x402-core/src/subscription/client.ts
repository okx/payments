import type { CancelAuth, ChargeResult, Subscription, SubscriptionCapability } from "./types";
import type { SubscriptionStore } from "./store";

export interface SubscriptionClientConfig {
  /**
   * The scheme instance to delegate facilitator-touching operations to.
   * Typically the same `PermitSubscriptionScheme` instance that is registered
   * to `x402ResourceServer`, so HTTP main-line state and out-of-band charge
   * state share one source of truth.
   */
  scheme: SubscriptionCapability;
  /**
   * The store the scheme writes to. Required here because `getSubscription`
   * reads from the store (fast path) while `syncFromChain` writes to it after
   * re-fetching from chain.
   */
  store: SubscriptionStore;
}

/**
 * Out-of-band primitives for Seller scheduler / business code. SDK ships the
 * per-call atoms (charge / cancelBySeller / syncFromChain / etc.) but NOT a
 * scheduler — cron, due index, retry policy, distributed locks are all
 * Seller infrastructure.
 *
 * All methods are thin wrappers:
 *   - `charge`           delegates to scheme.charge (throws ChargeError on fail)
 *   - `cancelBySeller`   delegates to scheme.settleCancel with a Seller-built
 *                        CancelAuth
 *   - `syncFromChain`    re-pulls Subscription via scheme.getSubscription and
 *                        repairs the store; connects with the `state==="changed"`
 *                        chain to also sync the downstream new sub
 *   - `getSubscription`  direct store read (does NOT touch the chain)
 */
export class SubscriptionClient {
  protected readonly scheme: SubscriptionCapability;
  protected readonly store: SubscriptionStore;

  constructor(config: SubscriptionClientConfig) {
    this.scheme = config.scheme;
    this.store = config.store;
  }

  /**
   * Run one charge period for a subscription. Throws `ChargeError` (one of 6
   * codes) on facilitator-side failure. Internally `scheme.charge` already
   * updates the store on success (and on `planChangeTriggered`); the client is
   * a pass-through.
   */
  async charge(subId: string): Promise<ChargeResult> {
    return this.scheme.charge(subId);
  }

  /**
   * Seller-initiated cancel (e.g. ToS violation, fraud, business reason).
   *
   * The SDK does NOT hold the Seller's merchant private key; the Seller must
   * construct + sign a `CancelAuth` with `by=1 (MERCHANT)` outside and pass
   * it in. SDK runs verifyCancel (sanity check on the auth) then settleCancel
   * (facilitator + store mark canceled).
   *
   * Throws on either verify or settle failure.
   */
  async cancelBySeller(subId: string, auth: CancelAuth, _reason?: string): Promise<void> {
    const v = await this.scheme.verifyCancel(auth, subId);
    if (!v.ok) {
      throw new Error(`cancelBySeller.verify failed: ${v.error}`);
    }
    const r = await this.scheme.settleCancel(auth, subId);
    if (!r.success) {
      throw new Error(`cancelBySeller.settle failed: ${r.error}`);
    }
  }

  /**
   * Re-sync a subscription from chain and repair the store. Use when:
   *   - `charge` threw `SubscriptionNotActive` (buyer may have cancelled
   *     directly via the facilitator or contract)
   *   - `charge` threw `ConfirmationTimeout` (network-level failure; chain
   *     may or may not have written)
   *   - periodic reconciliation
   *
   * If the synced sub is in `"changed"` state, the downstream `changedToSubId`
   * is also fetched and persisted, so the Seller's `dueIndex` can switch over
   * to the new sub without manual intervention.
   */
  async syncFromChain(subId: string): Promise<Subscription | null> {
    const latest = await this.scheme.getSubscription(subId);
    if (!latest) return null;
    await this.store.put(latest);

    if (latest.state === "changed" && latest.changedToSubId) {
      const newSub = await this.scheme.getSubscription(latest.changedToSubId);
      if (newSub) await this.store.put(newSub);
    }
    return latest;
  }

  /**
   * Direct store read. Cheap; does NOT touch the chain. Use this for hot-path
   * lookups (e.g. resolving subId to plan/tier for business logic). For chain
   * state of record, use `syncFromChain`.
   */
  async getSubscription(subId: string): Promise<Subscription | null> {
    return this.store.get(subId);
  }
}
