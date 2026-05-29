import type { Handler, ProtocolAdapter } from "../types.js";

interface HTTPRequestContext {
  adapter: HTTPAdapter;
  path: string;
  method: string;
  paymentHeader?: string;
  routePattern?: string;
}

interface HTTPAdapter {
  getHeader(name: string): string | undefined;
  getMethod(): string;
  getPath(): string;
  getUrl(): string;
  getAcceptHeader(): string;
  getUserAgent(): string;
  getQueryParams?(): Record<string, string | string[]>;
  getQueryParam?(name: string): string | string[] | undefined;
  getBody?(): unknown;
}

interface HTTPResponseInstructions {
  status: number;
  headers: Record<string, string>;
  body?: unknown;
  isHtml?: boolean;
}

type HTTPProcessResult =
  | { type: "no-payment-required" }
  | { type: "payment-error"; response: HTTPResponseInstructions }
  | {
      type: "payment-verified";
      paymentPayload: unknown;
      paymentRequirements: unknown;
      declaredExtensions?: Record<string, unknown>;
    };

interface ProcessSettleSuccessResponse {
  success: true;
  headers: Record<string, string>;
  [key: string]: unknown;
}

interface ProcessSettleFailureResponse {
  success: false;
  headers: Record<string, string>;
  response: HTTPResponseInstructions;
  [key: string]: unknown;
}

type ProcessSettleResultResponse =
  | ProcessSettleSuccessResponse
  | ProcessSettleFailureResponse;

interface X402HTTPResourceServerLike {
  initialize(): Promise<void>;
  requiresPayment(context: HTTPRequestContext): boolean;
  processHTTPRequest(
    context: HTTPRequestContext,
    paywallConfig?: unknown,
  ): Promise<HTTPProcessResult>;
  processSettlement(
    paymentPayload: unknown,
    requirements: unknown,
    declaredExtensions?: Record<string, unknown>,
    transportContext?: unknown,
    settlementOverrides?: unknown,
  ): Promise<ProcessSettleResultResponse>;
}

interface X402ResourceServerLike {
  initialize?(): Promise<void>;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type X402HTTPResourceServerCtor = new (...args: any[]) => X402HTTPResourceServerLike;

class WebRequestAdapter implements HTTPAdapter {
  private readonly url: URL;
  constructor(private readonly request: Request) {
    this.url = new URL(request.url);
  }
  getHeader(name: string): string | undefined {
    return this.request.headers.get(name) ?? undefined;
  }
  getMethod(): string {
    return this.request.method;
  }
  getPath(): string {
    return this.url.pathname;
  }
  getUrl(): string {
    return this.request.url;
  }
  getAcceptHeader(): string {
    return this.request.headers.get("accept") ?? "";
  }
  getUserAgent(): string {
    return this.request.headers.get("user-agent") ?? "";
  }
  getQueryParams(): Record<string, string | string[]> {
    const out: Record<string, string | string[]> = {};
    for (const [k, v] of this.url.searchParams.entries()) {
      if (k in out) {
        const cur = out[k];
        out[k] = Array.isArray(cur) ? [...cur, v] : [cur, v];
      } else {
        out[k] = v;
      }
    }
    return out;
  }
  getQueryParam(name: string): string | string[] | undefined {
    return this.getQueryParams()[name];
  }
}

/** A single x402 payment option. */
export interface X402PaymentOption {
  scheme: string;
  payTo: string;
  price: string | number | { asset: string; amount: string };
  network: string;
  maxTimeoutSeconds?: number;
  extra?: Record<string, unknown>;
}

/**
 * Per-route x402 configuration. Two shapes are accepted:
 *   1. Flattened single payment option (common case):
 *      `{ scheme: "exact", payTo, price: "$0.01", network: "eip155:196" }`
 *   2. Explicit `accepts` (multi-scheme case):
 *      `{ accepts: [{...}, {...}], description: "..." }`
 */
export type X402AdapterRouteConfig =
  | (X402PaymentOption & {
      description?: string;
      mimeType?: string;
      extensions?: Record<string, unknown>;
      paywallConfig?: unknown;
    })
  | {
      accepts: X402PaymentOption | X402PaymentOption[];
      description?: string;
      mimeType?: string;
      extensions?: Record<string, unknown>;
      paywallConfig?: unknown;
    };

export interface X402AdapterConfig {
  /**
   * Pre-configured `x402ResourceServer` (facilitator + schemes registered,
   * routes excluded). The adapter constructs an internal
   * `x402HTTPResourceServer` from the `paymentRouter.routes` entries.
   */
  resourceServer: X402ResourceServerLike;
  /**
   * Constructor for `x402HTTPResourceServer` (from
   * `@okxweb3/x402-core/server`). Injected explicitly so the adapter does not
   * hard-import x402-core.
   */
  httpResourceServerCtor: X402HTTPResourceServerCtor;
  /** Detect priority (default 20 — runs after MPP). */
  priority?: number;
  /** Default `paywallConfig` for all routes; per-route `paywallConfig` wins. */
  paywallConfig?: unknown;
}

export class X402Adapter implements ProtocolAdapter<X402AdapterRouteConfig> {
  readonly name = "x402";
  readonly priority: number;
  private readonly resourceServer: X402ResourceServerLike;
  private readonly httpResourceServerCtor: X402HTTPResourceServerCtor;
  private readonly defaultPaywallConfig?: unknown;
  private httpServer: X402HTTPResourceServerLike | null = null;
  private initialized = false;
  private initPromise: Promise<void> | null = null;

  constructor(cfg: X402AdapterConfig) {
    this.resourceServer = cfg.resourceServer;
    this.httpResourceServerCtor = cfg.httpResourceServerCtor;
    this.priority = cfg.priority ?? 20;
    this.defaultPaywallConfig = cfg.paywallConfig;
  }

  prepare(routes: ReadonlyMap<string, X402AdapterRouteConfig>): void {
    const x402Routes: Record<string, unknown> = {};
    for (const [pattern, cfg] of routes) {
      x402Routes[pattern] = toX402RouteConfig(cfg);
    }
    this.httpServer = new this.httpResourceServerCtor(
      this.resourceServer,
      x402Routes,
    );
  }

  async initialize(): Promise<void> {
    if (this.initialized) return;
    if (!this.httpServer) {
      throw new Error(
        "X402Adapter: prepare() must be called before initialize()",
      );
    }
    if (!this.initPromise) {
      this.initPromise = this.httpServer.initialize();
    }
    try {
      await this.initPromise;
      this.initialized = true;
    } catch (err) {
      this.initPromise = null;
      throw err;
    }
  }

  detect(request: Request): boolean {
    return !!(
      request.headers.get("payment-signature") ||
      request.headers.get("x-payment")
    );
  }

  async buildChallenge(
    request: Request,
    routeConfig: X402AdapterRouteConfig,
  ): Promise<Record<string, string>> {
    const httpServer = this.requireHttpServer();
    const ctx = makeContext(request);
    const result = await httpServer.processHTTPRequest(
      ctx,
      pickPaywallConfig(routeConfig) ?? this.defaultPaywallConfig,
    );
    if (result.type === "payment-error") {
      return { ...result.response.headers };
    }
    return {};
  }

  async handle(
    request: Request,
    routeConfig: X402AdapterRouteConfig,
    inner: Handler,
  ): Promise<Response> {
    const httpServer = this.requireHttpServer();
    const ctx = makeContext(request);
    const paywallConfig =
      pickPaywallConfig(routeConfig) ?? this.defaultPaywallConfig;

    const verifyResult = await httpServer.processHTTPRequest(ctx, paywallConfig);

    if (verifyResult.type === "no-payment-required") {
      return inner(request);
    }

    if (verifyResult.type === "payment-error") {
      return instructionsToResponse(verifyResult.response);
    }

    const innerResp = await inner(request);

    if (innerResp.status >= 400) return innerResp;

    const bodyBuf = Buffer.from(await innerResp.clone().arrayBuffer());

    const settle = await httpServer.processSettlement(
      verifyResult.paymentPayload,
      verifyResult.paymentRequirements,
      verifyResult.declaredExtensions,
      { request: ctx, responseBody: bodyBuf },
    );

    if (!settle.success) {
      return instructionsToResponse(settle.response);
    }

    const merged = new Headers(innerResp.headers);
    for (const [k, v] of Object.entries(settle.headers)) {
      merged.set(k, v);
    }
    return new Response(innerResp.body, {
      status: innerResp.status,
      statusText: innerResp.statusText,
      headers: merged,
    });
  }

  private requireHttpServer(): X402HTTPResourceServerLike {
    if (!this.httpServer) {
      throw new Error(
        "X402Adapter: prepare() was not called; did you forget to register the adapter on paymentRouter?",
      );
    }
    return this.httpServer;
  }
}

function toX402RouteConfig(cfg: X402AdapterRouteConfig): unknown {
  if ("accepts" in cfg) {
    const { paywallConfig: _ignored, ...rest } = cfg;
    return rest;
  }
  const {
    scheme,
    payTo,
    price,
    network,
    maxTimeoutSeconds,
    extra,
    description,
    mimeType,
    extensions,
    paywallConfig: _ignored,
  } = cfg;
  return {
    accepts: { scheme, payTo, price, network, maxTimeoutSeconds, extra },
    description,
    mimeType,
    extensions,
  };
}

function pickPaywallConfig(cfg: X402AdapterRouteConfig): unknown {
  return (cfg as { paywallConfig?: unknown }).paywallConfig;
}

function makeContext(request: Request): HTTPRequestContext {
  const adapter = new WebRequestAdapter(request);
  return {
    adapter,
    path: new URL(request.url).pathname,
    method: request.method,
    paymentHeader:
      adapter.getHeader("payment-signature") ?? adapter.getHeader("x-payment"),
  };
}

function instructionsToResponse(r: HTTPResponseInstructions): Response {
  const headers = new Headers(r.headers);
  if (r.isHtml) {
    if (!headers.has("content-type")) headers.set("content-type", "text/html");
    return new Response(typeof r.body === "string" ? r.body : "", {
      status: r.status,
      headers,
    });
  }
  if (!headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  const body = r.body == null ? "{}" : JSON.stringify(r.body);
  return new Response(body, { status: r.status, headers });
}
