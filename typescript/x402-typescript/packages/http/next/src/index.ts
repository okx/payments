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
  RouteConfig,
} from "@okxweb3/app-x402-core/server";
import { SchemeNetworkServer, Network } from "@okxweb3/app-x402-core/types";
import { NextRequest, NextResponse } from "next/server";
import { NextAdapter } from "./adapter";

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
 *
 * @param error - The facilitator response error to surface
 * @returns A NextResponse with 502 status
 */
function createFacilitatorErrorResponse(error: FacilitatorResponseError): NextResponse {
  return new NextResponse(JSON.stringify({ error: error.message }), {
    status: 502,
    headers: { "Content-Type": "application/json" },
  });
}

/**
 * Prepare HTTP server with initialization tracking.
 */
function prepareHttpServer(
  httpServer: x402HTTPResourceServer,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
) {
  if (paywall) {
    httpServer.registerPaywallProvider(paywall);
  }

  let initPromise: Promise<void> | null = syncFacilitatorOnStart ? httpServer.initialize() : null;
  let isInitialized = false;

  return {
    httpServer,
    /**
     * Ensures facilitator initialization succeeds once, while allowing retries after failures.
     */
    async init(): Promise<void> {
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
    },
  };
}

/**
 * Create HTTP request context from NextRequest.
 */
function createRequestContext(request: NextRequest): HTTPRequestContext {
  const adapter = new NextAdapter(request);
  return {
    adapter,
    path: request.nextUrl.pathname,
    method: request.method,
    paymentHeader: adapter.getHeader("payment-signature") || adapter.getHeader("x-payment"),
  };
}

/**
 * Handle payment error result and create appropriate NextResponse.
 */
function handlePaymentError(response: {
  status: number;
  headers: Record<string, string>;
  body?: unknown;
  isHtml?: boolean;
}): NextResponse {
  const headers = new Headers(response.headers);
  if (response.isHtml) {
    headers.set("Content-Type", "text/html");
    return new NextResponse(response.body as string, {
      status: response.status,
      headers,
    });
  }
  headers.set("Content-Type", "application/json");
  return new NextResponse(JSON.stringify(response.body || {}), {
    status: response.status,
    headers,
  });
}

/**
 * Handle settlement after handler response.
 */
async function handleSettlement(
  httpServer: x402HTTPResourceServer,
  response: NextResponse,
  paymentPayload: Parameters<x402HTTPResourceServer["processSettlement"]>[0],
  paymentRequirements: Parameters<x402HTTPResourceServer["processSettlement"]>[1],
  declaredExtensions: Parameters<x402HTTPResourceServer["processSettlement"]>[2],
  httpContext: HTTPRequestContext,
): Promise<NextResponse> {
  if (response.status >= 400) {
    return response;
  }

  try {
    const responseBody = Buffer.from(await response.clone().arrayBuffer());

    const result = await httpServer.processSettlement(
      paymentPayload,
      paymentRequirements,
      declaredExtensions,
      { request: httpContext, responseBody },
    );

    if (!result.success) {
      const { response: settleResponse } = result;
      const body = settleResponse.isHtml
        ? (settleResponse.body as string)
        : JSON.stringify(settleResponse.body ?? {});
      return new NextResponse(body, {
        status: settleResponse.status,
        headers: settleResponse.headers,
      });
    }

    Object.entries(result.headers).forEach(([key, value]) => {
      response.headers.set(key, value);
    });
    return response;
  } catch (error) {
    if (error instanceof FacilitatorResponseError) {
      return createFacilitatorErrorResponse(error);
    }
    console.error("Settlement failed:", error);
    return new NextResponse(JSON.stringify({}), {
      status: 402,
      headers: { "Content-Type": "application/json" },
    });
  }
}

/**
 * Next.js payment proxy for x402 protocol (direct HTTP server instance).
 *
 * Use this when you need to configure HTTP-level hooks.
 *
 * @param httpServer - Pre-configured x402HTTPResourceServer instance
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns Next.js proxy handler
 *
 * @example
 * ```typescript
 * import { paymentProxyFromHTTPServer, x402ResourceServer, x402HTTPResourceServer } from "@okxweb3/app-x402-next";
 *
 * const resourceServer = new x402ResourceServer(facilitatorClient)
 *   .register(NETWORK, new ExactEvmScheme());
 *
 * const httpServer = new x402HTTPResourceServer(resourceServer, routes)
 *   .onProtectedRequest(requestHook);
 *
 * export const proxy = paymentProxyFromHTTPServer(httpServer);
 * ```
 */
export function paymentProxyFromHTTPServer(
  httpServer: x402HTTPResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): (req: NextRequest) => Promise<NextResponse> {
  const { init } = prepareHttpServer(httpServer, paywall, syncFacilitatorOnStart);

  return async (req: NextRequest) => {
    const context = createRequestContext(req);

    if (!httpServer.requiresPayment(context)) {
      return NextResponse.next();
    }

    try {
      await init();
    } catch (error) {
      const facilitatorError = getFacilitatorResponseError(error);
      if (facilitatorError) {
        return createFacilitatorErrorResponse(facilitatorError);
      }
      throw error;
    }

    let result: Awaited<ReturnType<x402HTTPResourceServer["processHTTPRequest"]>>;
    try {
      result = await httpServer.processHTTPRequest(context, paywallConfig);
    } catch (error) {
      if (error instanceof FacilitatorResponseError) {
        return createFacilitatorErrorResponse(error);
      }
      throw error;
    }

    switch (result.type) {
      case "no-payment-required":
        return NextResponse.next();

      case "payment-error":
        return handlePaymentError(result.response);

      case "payment-verified": {
        const { paymentPayload, paymentRequirements, declaredExtensions } = result;
        const nextResponse = NextResponse.next();
        return handleSettlement(
          httpServer,
          nextResponse,
          paymentPayload,
          paymentRequirements,
          declaredExtensions,
          context,
        );
      }
    }
  };
}

/**
 * Next.js payment proxy for x402 protocol (direct server instance).
 *
 * @param routes - Route configurations for protected endpoints
 * @param server - Pre-configured x402ResourceServer instance
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns Next.js proxy handler
 *
 * @example
 * ```typescript
 * import { paymentProxy } from "@okxweb3/app-x402-next";
 *
 * const server = new x402ResourceServer(myFacilitatorClient)
 *   .register(NETWORK, new ExactEvmScheme());
 *
 * export const proxy = paymentProxy(routes, server, paywallConfig);
 * ```
 */
export function paymentProxy(
  routes: RoutesConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): (req: NextRequest) => Promise<NextResponse> {
  const httpServer = new x402HTTPResourceServer(server, routes);
  return paymentProxyFromHTTPServer(httpServer, paywallConfig, paywall, syncFacilitatorOnStart);
}

/**
 * Next.js payment proxy for x402 protocol (configuration-based).
 *
 * @param routes - Route configurations for protected endpoints
 * @param facilitatorClients - Optional facilitator client(s) for payment processing
 * @param schemes - Optional array of scheme registrations for server-side payment processing
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns Next.js proxy handler
 *
 * @example
 * ```typescript
 * import { paymentProxyFromConfig } from "@okxweb3/app-x402-next";
 *
 * export const proxy = paymentProxyFromConfig(
 *   routes,
 *   myFacilitatorClient,
 *   [{ network: "eip155:196", server: evmSchemeServer }],
 *   paywallConfig
 * );
 * ```
 */
export function paymentProxyFromConfig(
  routes: RoutesConfig,
  facilitatorClients?: FacilitatorClient | FacilitatorClient[],
  schemes?: SchemeRegistration[],
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): (req: NextRequest) => Promise<NextResponse> {
  const ResourceServer = new x402ResourceServer(facilitatorClients);

  if (schemes) {
    schemes.forEach(({ network, server: schemeServer }) => {
      ResourceServer.register(network, schemeServer);
    });
  }

  return paymentProxy(routes, ResourceServer, paywallConfig, paywall, syncFacilitatorOnStart);
}

/**
 * Wraps a Next.js App Router API route handler with x402 payment protection (HTTP server instance).
 *
 * @param routeHandler - The API route handler function to wrap
 * @param httpServer - Pre-configured x402HTTPResourceServer instance
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns A wrapped Next.js route handler
 *
 * @example
 * ```typescript
 * import { NextRequest, NextResponse } from "next/server";
 * import { withX402FromHTTPServer, x402ResourceServer, x402HTTPResourceServer } from "@okxweb3/app-x402-next";
 *
 * const resourceServer = new x402ResourceServer(facilitatorClient)
 *   .register(NETWORK, new ExactEvmScheme());
 *
 * const httpServer = new x402HTTPResourceServer(resourceServer, { "*": routeConfig })
 *   .onProtectedRequest(requestHook);
 *
 * const handler = async (request: NextRequest) => {
 *   return NextResponse.json({ data: "protected content" });
 * };
 *
 * export const GET = withX402FromHTTPServer(handler, httpServer);
 * ```
 */
export function withX402FromHTTPServer<T = unknown>(
  routeHandler: (request: NextRequest) => Promise<NextResponse<T>>,
  httpServer: x402HTTPResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): (request: NextRequest) => Promise<NextResponse<T>> {
  const { init } = prepareHttpServer(httpServer, paywall, syncFacilitatorOnStart);

  return async (request: NextRequest): Promise<NextResponse<T>> => {
    await init();

    const context = createRequestContext(request);
    const result = await httpServer.processHTTPRequest(context, paywallConfig);

    switch (result.type) {
      case "no-payment-required":
        return routeHandler(request);

      case "payment-error":
        return handlePaymentError(result.response) as NextResponse<T>;

      case "payment-verified": {
        const { paymentPayload, paymentRequirements, declaredExtensions } = result;
        const handlerResponse = await routeHandler(request);
        return handleSettlement(
          httpServer,
          handlerResponse,
          paymentPayload,
          paymentRequirements,
          declaredExtensions,
          context,
        ) as Promise<NextResponse<T>>;
      }
    }
  };
}

/**
 * Wraps a Next.js App Router API route handler with x402 payment protection.
 *
 * Unlike `paymentProxy` which works as middleware, `withX402` wraps individual route handlers
 * and guarantees that payment settlement only occurs after the handler returns a successful
 * response (status < 400).
 *
 * @param routeHandler - The API route handler function to wrap
 * @param routeConfig - Payment configuration for this specific route
 * @param server - Pre-configured x402ResourceServer instance
 * @param paywallConfig - Optional configuration for the built-in paywall UI
 * @param paywall - Optional custom paywall provider (overrides default)
 * @param syncFacilitatorOnStart - Whether to sync with the facilitator on startup (defaults to true)
 * @returns A wrapped Next.js route handler
 *
 * @example
 * ```typescript
 * import { NextRequest, NextResponse } from "next/server";
 * import { withX402 } from "@okxweb3/app-x402-next";
 *
 * const server = new x402ResourceServer(myFacilitatorClient)
 *   .register(NETWORK, new ExactEvmScheme());
 *
 * const handler = async (request: NextRequest) => {
 *   return NextResponse.json({ data: "protected content" });
 * };
 *
 * export const GET = withX402(
 *   handler,
 *   {
 *     accepts: {
 *       scheme: "exact",
 *       payTo: "0x123...",
 *       price: "$0.01",
 *       network: "eip155:196",
 *     },
 *     description: "Access to protected API",
 *   },
 *   server,
 * );
 * ```
 */
export function withX402<T = unknown>(
  routeHandler: (request: NextRequest) => Promise<NextResponse<T>>,
  routeConfig: RouteConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart: boolean = true,
): (request: NextRequest) => Promise<NextResponse<T>> {
  const routes = { "*": routeConfig };
  const httpServer = new x402HTTPResourceServer(server, routes);
  return withX402FromHTTPServer(
    routeHandler,
    httpServer,
    paywallConfig,
    paywall,
    syncFacilitatorOnStart,
  );
}

export { x402ResourceServer, x402HTTPResourceServer } from "@okxweb3/app-x402-core/server";

export type {
  PaymentRequired,
  PaymentRequirements,
  PaymentPayload,
  Network,
  SchemeNetworkServer,
} from "@okxweb3/app-x402-core/types";

export type { PaywallProvider, PaywallConfig, RouteConfig } from "@okxweb3/app-x402-core/server";

export { RouteConfigurationError } from "@okxweb3/app-x402-core/server";

export type { RouteValidationError } from "@okxweb3/app-x402-core/server";

export { NextAdapter } from "./adapter";
