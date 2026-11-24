import type { Handler, ProtocolAdapter } from "../types.js";

/**
 * Shape of a single mppx method handler:
 *   `mppx.charge(opts)(request)` returns either
 *     - `{ status: 402, challenge: Response }`                  — no payment, return the 402.
 *     - `{ status: 200, withReceipt: <T>(resp: T) => T }`       — verified, wrap the business response.
 *
 * Kept as a structural type to avoid hard-coupling to mppx's concrete types.
 */
type MppxMethodHandler = (
  options: Record<string, unknown>,
) => (
  request: Request,
) => Promise<
  | { status: 402; challenge: Response }
  | { status: 200; withReceipt: <T>(response: T) => T }
>;

/** Minimal structural shape of the instance returned by `Mppx.create({ methods: [...] })`. */
interface MppxInstance {
  charge?: MppxMethodHandler;
  session?: MppxMethodHandler;
  [key: string]: unknown;
}

/**
 * Per-route MPP configuration.
 *
 * All fields are forwarded as-is to `mppx.{intent}(opts)`; the mppx schema
 * decides which are required. Intentionally loosely typed to avoid duplicating
 * the protocol schema inside the middleware layer.
 */
export interface MppAdapterRouteConfig {
  /** Which method to invoke (`charge` / `session` / custom). Defaults to `"charge"`. */
  intent?: string;
  /** Forwarded to the mppx method as options. */
  [key: string]: unknown;
}

export interface MppAdapterConfig {
  /** The instance returned by `Mppx.create(...)`. */
  mppx: MppxInstance;
  /** Detect priority (default 10 — runs before x402). */
  priority?: number;
  /** Fallback intent when a route does not specify one (default `"charge"`). */
  defaultIntent?: string;
}

/**
 * MPP protocol adapter.
 *
 *  - `detect`         — looks for `Authorization: Payment ...`.
 *  - `buildChallenge` — invokes mppx with no credentials and extracts
 *                       `WWW-Authenticate` from the resulting 402.
 *  - `handle`         — invokes mppx; on 402 forwards the challenge, on 200
 *                       calls the inner handler and wraps the response with
 *                       `withReceipt`.
 */
export class MppAdapter implements ProtocolAdapter<MppAdapterRouteConfig> {
  readonly name = "mpp";
  readonly priority: number;
  private readonly mppx: MppxInstance;
  private readonly defaultIntent: string;

  constructor(cfg: MppAdapterConfig) {
    this.mppx = cfg.mppx;
    this.priority = cfg.priority ?? 10;
    this.defaultIntent = cfg.defaultIntent ?? "charge";
  }

  detect(request: Request): boolean {
    const auth = request.headers.get("authorization");
    return !!auth && /^Payment\s+/i.test(auth);
  }

  async buildChallenge(
    request: Request,
    routeConfig: MppAdapterRouteConfig,
  ): Promise<Record<string, string>> {
    const { handler, options } = this.resolveHandler(routeConfig);
    if (!handler) return {};

    const probe = stripAuthorization(request);
    const result = await handler(options)(probe);
    if (result.status !== 402) return {};

    const headers: Record<string, string> = {};
    for (const [k, v] of result.challenge.headers) {
      if (k.toLowerCase() === "www-authenticate") {
        headers["WWW-Authenticate"] = v;
      }
    }
    return headers;
  }

  async handle(
    request: Request,
    routeConfig: MppAdapterRouteConfig,
    inner: Handler,
  ): Promise<Response> {
    const { handler, options } = this.resolveHandler(routeConfig);
    if (!handler) {
      return new Response(
        JSON.stringify({ error: `mpp intent not registered` }),
        { status: 500, headers: { "content-type": "application/json" } },
      );
    }

    const result = await handler(options)(request);
    if (result.status === 402) {
      return result.challenge;
    }

    const innerResp = await inner(request);
    return result.withReceipt(innerResp);
  }

  private resolveHandler(routeConfig: MppAdapterRouteConfig): {
    handler: MppxMethodHandler | undefined;
    options: Record<string, unknown>;
  } {
    const { intent: rawIntent, ...rest } = routeConfig ?? {};
    const intent = (rawIntent as string | undefined) ?? this.defaultIntent;
    return {
      handler: this.mppx[intent] as MppxMethodHandler | undefined,
      options: rest,
    };
  }
}

function stripAuthorization(request: Request): Request {
  const headers = new Headers(request.headers);
  headers.delete("authorization");
  return new Request(request.url, { method: request.method, headers });
}
