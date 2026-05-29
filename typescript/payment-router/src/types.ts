/**
 * Unified payment middleware — type definitions.
 */

/** User business handler — called after payment verification succeeds. */
export type Handler = (request: Request) => Promise<Response> | Response;

/**
 * Protocol adapter interface.
 *
 * Methods:
 *  - `detect(request)`: header-only check for whether the request carries this
 *    protocol's credentials; must be side-effect-free and fast.
 *  - `buildChallenge(request, cfg)`: invoked concurrently by the middleware
 *    when no protocol matches, the returned headers are merged into the 402
 *    response.
 *  - `handle(request, cfg, inner)`: invoked when the request matches this
 *    protocol. The adapter performs verify → call `inner(request)` for the
 *    business response → settle → attach receipt. May return a 402/4xx
 *    response directly; the middleware forwards it as-is.
 */
export interface ProtocolAdapter<Cfg = unknown> {
  /** Unique protocol identifier (e.g. `"mpp"`, `"x402"`, or custom). */
  readonly name: string;

  /** Detect ordering — lower runs first (MPP=10, x402=20). */
  readonly priority: number;

  /** One-shot async initialization (idempotent). */
  initialize?(): Promise<void>;

  /**
   * Optional hook invoked once before the middleware first runs, receiving
   * all per-route configs declared for this adapter.
   *
   * Called before `initialize()` and exactly once.
   */
  prepare?(routes: ReadonlyMap<string, Cfg>): void | Promise<void>;

  /** Does the request carry this protocol's credentials? */
  detect(request: Request): boolean;

  /** Build response headers for the 402 challenge (status excluded). */
  buildChallenge(
    request: Request,
    config: Cfg,
  ): Promise<Record<string, string>>;

  /** Run the full verify → business → settle pipeline and return the final response. */
  handle(
    request: Request,
    config: Cfg,
    inner: Handler,
  ): Promise<Response>;
}

/** Per-adapter config container for a single route. */
export interface UnifiedRouteConfig {
  description?: string;
  /**
   * `Map<adapterName, configForThatAdapter>`. The key must exist for the
   * matching adapter to participate in challenge / handle on this route.
   * A value of `{}` means "enable, but use the adapter's own preconfigured
   * defaults".
   */
  adapterConfigs: Record<string, unknown>;
}

export type UnifiedRoutesConfig = Record<string, UnifiedRouteConfig>;

/** Top-level middleware configuration. */
export interface PaymentRouterConfig {
  adapters: ProtocolAdapter[];
  routes: UnifiedRoutesConfig;
  /** Called when a single adapter's `buildChallenge` throws; other adapters are unaffected. */
  onError?: (error: unknown, protocol: string) => void;
}
