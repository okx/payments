import type { ProtocolAdapter } from "./types.js";

/**
 * Protocol detector: walks adapters in ascending priority order; the first
 * adapter whose `detect()` returns true wins. Pure detection — no side effects
 * and no verification.
 */
export class ProtocolDetector {
  private readonly sorted: ProtocolAdapter[];

  constructor(adapters: readonly ProtocolAdapter[]) {
    this.sorted = [...adapters].sort((a, b) => a.priority - b.priority);
  }

  /** Returns the matched adapter, or `null` if none matched. */
  detect(request: Request): ProtocolAdapter | null {
    for (const a of this.sorted) {
      if (a.detect(request)) return a;
    }
    return null;
  }

  get adapters(): readonly ProtocolAdapter[] {
    return this.sorted;
  }
}
