import { ProtocolDetector } from "./detector.js";
import { mergeChallenges } from "./merger.js";
import { compileRoutes, matchRoute, type CompiledRoute } from "./router.js";
import type {
  Handler,
  PaymentRouterConfig,
  ProtocolAdapter,
} from "./types.js";

/**
 * Create a unified payment middleware.
 *
 * Returns `(inner: Handler) => Handler`:
 *   - Path not declared in `routes`               → pass through to `inner`.
 *   - Matched route, no credentials in request    → collect all adapter
 *                                                   challenges concurrently
 *                                                   and return 402.
 *   - Matched route, a credential is detected     → hand off to that
 *                                                   adapter's `handle()`.
 *
 * @example
 * ```ts
 * const protect = paymentRouter({
 *   adapters: [mppAdapter, x402Adapter],
 *   routes: {
 *     "GET /generateImg": {
 *       description: "AI Image Generation Service",
 *       adapterConfigs: {
 *         mpp: { amount: "10000", description: "AI Image Generation Service" },
 *         x402: {},
 *       },
 *     },
 *   },
 * })
 *
 * const handler = protect(async (req) => Response.json({ imageUrl: "..." }))
 * ```
 */
export function paymentRouter(config: PaymentRouterConfig) {
  const compiled: CompiledRoute[] = compileRoutes(config.routes);
  const adapters: ProtocolAdapter[] = [...config.adapters].sort(
    (a, b) => a.priority - b.priority,
  );
  const detector = new ProtocolDetector(adapters);

  let initPromise: Promise<void> | null = null;
  let initialized = false;

  async function ensureInitialized(): Promise<void> {
    if (initialized) return;
    if (!initPromise) {
      initPromise = (async () => {
        for (const a of adapters) {
          if (!a.prepare) continue;
          const adapterRoutes = collectAdapterRoutes(config.routes, a.name);
          await a.prepare(adapterRoutes);
        }
        await Promise.all(
          adapters.map((a) => a.initialize?.() ?? Promise.resolve()),
        );
        initialized = true;
      })();
    }
    try {
      await initPromise;
    } catch (err) {
      initPromise = null;
      throw err;
    }
  }

  return function withPayment(inner: Handler): Handler {
    return async (request: Request): Promise<Response> => {
      const url = new URL(request.url);
      const routeConfig = matchRoute(compiled, url.pathname, request.method);

      if (!routeConfig) return inner(request);

      await ensureInitialized();

      const matched = detector.detect(request);
      if (matched) {
        const adapterCfg = routeConfig.adapterConfigs[matched.name];
        if (adapterCfg == null) {
          return buildChallengeResponse(request, routeConfig, adapters, config);
        }
        return matched.handle(request, adapterCfg, inner);
      }

      return buildChallengeResponse(request, routeConfig, adapters, config);
    };
  };
}

/**
 * Extract one adapter's per-route configs from the full routes map.
 * Consumed by `adapter.prepare(routes)`.
 */
function collectAdapterRoutes(
  routes: import("./types.js").UnifiedRoutesConfig,
  adapterName: string,
): ReadonlyMap<string, unknown> {
  const out = new Map<string, unknown>();
  for (const [pattern, route] of Object.entries(routes)) {
    const cfg = route.adapterConfigs?.[adapterName];
    if (cfg != null) out.set(pattern, cfg);
  }
  return out;
}

async function buildChallengeResponse(
  request: Request,
  routeConfig: import("./types.js").UnifiedRouteConfig,
  adapters: readonly ProtocolAdapter[],
  config: PaymentRouterConfig,
): Promise<Response> {
  const headers = await mergeChallenges(
    request,
    routeConfig,
    adapters,
    config.onError,
  );

  const responseHeaders = new Headers(headers);
  if (!responseHeaders.has("content-type")) {
    responseHeaders.set("content-type", "application/json");
  }

  return new Response(JSON.stringify({ error: "Payment required" }), {
    status: 402,
    headers: responseHeaders,
  });
}
