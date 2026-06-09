import {
  HTTPRequestContext,
  PaywallConfig,
  PaywallProvider,
  x402HTTPResourceServer,
  x402ResourceServer,
  RoutesConfig,
  FacilitatorClient,
  FacilitatorResponseError,
  getFacilitatorResponseError,
} from "@okxweb3/app-x402-core/server";
import { SchemeNetworkServer, Network } from "@okxweb3/app-x402-core/types";
import type { Context, MiddlewareHandler } from "hono";
import { HonoAdapter } from "./adapter";

/**
 * Configuration for registering a payment scheme with a specific network
 */
export interface SchemeRegistration {
  /**
   * The network identifier (e.g., 'eip155:196', 'eip155:196')
   */
  network: Network;

  /**
   * The scheme server implementation for this network
   */
  server: SchemeNetworkServer;
}

/**
 * Sends a normalized 502 response for facilitator boundary failures.
 */
function facilitatorErrorResponse(c: Context, error: FacilitatorResponseError): Response {
  return c.json({ error: error.message }, 502);
}

/**
 * Hono payment middleware for x402 protocol (direct HTTP server instance).
 *
 * Use this when you need to configure HTTP-level hooks.
 *
 * @param httpServer - Pre-configured x402HTTPResourceServer instance
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns Hono middleware handler
 *
 * @example
 * ```typescript
 * import { paymentMiddlewareFromHTTPServer, x402ResourceServer, x402HTTPResourceServer } from "@okxweb3/app-x402-hono";
 *
 * const resourceServer = new x402ResourceServer(facilitatorClient)
 *   .register(NETWORK, new ExactEvmScheme());
 *
 * const httpServer = new x402HTTPResourceServer(resourceServer, routes)
 *   .onProtectedRequest(requestHook);
 *
 * app.use(paymentMiddlewareFromHTTPServer(httpServer));
 * ```
 */
export function paymentMiddlewareFromHTTPServer(
  httpServer: x402HTTPResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): MiddlewareHandler {
  if (paywall) {
    httpServer.registerPaywallProvider(paywall);
  }

  let initPromise: Promise<void> | null = syncFacilitatorOnStart ? httpServer.initialize() : null;
  let isInitialized = false;

  async function initializeHttpServer(): Promise<void> {
    if (!syncFacilitatorOnStart || isInitialized) {
      return;
    }

    if (!initPromise) {
      initPromise = httpServer.initialize();
    }

    try {
      await initPromise;
      isInitialized = true;
    } catch (error) {
      initPromise = null;
      throw error;
    }
  }

  return async (c, next) => {
    const adapter = new HonoAdapter(c);
    const context: HTTPRequestContext = {
      adapter,
      path: c.req.path,
      method: c.req.method,
      paymentHeader: adapter.getHeader("payment-signature") || adapter.getHeader("x-payment"),
    };

    if (!httpServer.requiresPayment(context)) {
      return next();
    }

    if (syncFacilitatorOnStart && !isInitialized) {
      try {
        await initializeHttpServer();
      } catch (error) {
        const facError = getFacilitatorResponseError(error);
        if (facError) {
          return facilitatorErrorResponse(c, facError);
        }
        throw error;
      }
    }

    let result: Awaited<ReturnType<x402HTTPResourceServer["processHTTPRequest"]>>;
    try {
      result = await httpServer.processHTTPRequest(context, paywallConfig);
    } catch (error) {
      if (error instanceof FacilitatorResponseError) {
        return facilitatorErrorResponse(c, error);
      }
      throw error;
    }

    switch (result.type) {
      case "no-payment-required":
        return next();

      case "payment-error": {
        const { response } = result;
        Object.entries(response.headers).forEach(([key, value]) => {
          c.header(key, value);
        });
        if (response.isHtml) {
          return c.html(response.body as string, response.status as 402);
        } else {
          return c.json(response.body || {}, response.status as 402);
        }
      }

      case "payment-verified": {
        const { paymentPayload, paymentRequirements, declaredExtensions } = result;

        await next();

        let res = c.res;

        if (res.status >= 400) {
          return;
        }

        const responseBody = Buffer.from(await res.clone().arrayBuffer());
        c.res = undefined!;

        try {
          const settleResult = await httpServer.processSettlement(
            paymentPayload,
            paymentRequirements,
            declaredExtensions,
            { request: context, responseBody },
          );

          if (!settleResult.success) {
            const { response: settleResponse } = settleResult;
            const body = settleResponse.isHtml
              ? String(settleResponse.body ?? "")
              : JSON.stringify(settleResponse.body ?? {});
            res = new Response(body, {
              status: settleResponse.status,
              headers: settleResponse.headers,
            });
          } else {
            Object.entries(settleResult.headers).forEach(([key, value]) => {
              res.headers.set(key, value);
            });
          }
        } catch (error) {
          if (error instanceof FacilitatorResponseError) {
            res = facilitatorErrorResponse(c, error);
            c.res = res;
            return;
          }
          console.error(error);
          res = c.json({}, 402);
        }

        c.res = res;
        return;
      }
    }
  };
}

/**
 * Hono payment middleware for x402 protocol (direct server instance).
 *
 * @param routes - Route configurations for protected endpoints
 * @param server - Pre-configured x402ResourceServer instance
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns Hono middleware handler
 *
 * @example
 * ```typescript
 * import { paymentMiddleware } from "@okxweb3/app-x402-hono";
 *
 * const server = new x402ResourceServer(myFacilitatorClient)
 *   .register(NETWORK, new ExactEvmScheme());
 *
 * app.use(paymentMiddleware(routes, server, paywallConfig));
 * ```
 */
export function paymentMiddleware(
  routes: RoutesConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): MiddlewareHandler {
  const httpServer = new x402HTTPResourceServer(server, routes);
  return paymentMiddlewareFromHTTPServer(
    httpServer,
    paywallConfig,
    paywall,
    syncFacilitatorOnStart,
  );
}

/**
 * Hono payment middleware for x402 protocol (configuration-based).
 *
 * @param routes - Route configurations for protected endpoints
 * @param facilitatorClients - Optional facilitator client(s) for payment processing
 * @param schemes - Optional array of scheme registrations for server-side payment processing
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns Hono middleware handler
 *
 * @example
 * ```typescript
 * import { paymentMiddlewareFromConfig } from "@okxweb3/app-x402-hono";
 *
 * app.use(paymentMiddlewareFromConfig(
 *   routes,
 *   myFacilitatorClient,
 *   [{ network: "eip155:196", server: evmSchemeServer }],
 *   paywallConfig
 * ));
 * ```
 */
export function paymentMiddlewareFromConfig(
  routes: RoutesConfig,
  facilitatorClients?: FacilitatorClient | FacilitatorClient[],
  schemes?: SchemeRegistration[],
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): MiddlewareHandler {
  const ResourceServer = new x402ResourceServer(facilitatorClients);

  if (schemes) {
    schemes.forEach(({ network, server: schemeServer }) => {
      ResourceServer.register(network, schemeServer);
    });
  }

  return paymentMiddleware(routes, ResourceServer, paywallConfig, paywall, syncFacilitatorOnStart);
}

export { x402ResourceServer, x402HTTPResourceServer } from "@okxweb3/app-x402-core/server";

export type {
  PaymentRequired,
  PaymentRequirements,
  PaymentPayload,
  Network,
  SchemeNetworkServer,
} from "@okxweb3/app-x402-core/types";

export type { PaywallProvider, PaywallConfig } from "@okxweb3/app-x402-core/server";

export { RouteConfigurationError } from "@okxweb3/app-x402-core/server";

export type { RouteValidationError } from "@okxweb3/app-x402-core/server";

export { HonoAdapter } from "./adapter";
