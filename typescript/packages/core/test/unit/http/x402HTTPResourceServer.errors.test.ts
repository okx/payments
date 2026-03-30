import { beforeEach, describe, expect, it } from "vitest";
import {
  x402HTTPResourceServer,
  HTTPAdapter,
  RouteConfigurationError,
  RouteValidationError,
} from "../../../src/http/x402HTTPResourceServer";
import { x402ResourceServer } from "../../../src/server/x402ResourceServer";
import { FacilitatorResponseError, Network } from "../../../src/types";
import {
  MockFacilitatorClient,
  MockSchemeNetworkServer,
  buildPaymentPayload,
  buildPaymentRequirements,
  buildSupportedResponse,
} from "../../mocks";
import { encodePaymentSignatureHeader } from "../../../src/http";

class MockHTTPAdapter implements HTTPAdapter {
  constructor(private readonly headers: Record<string, string> = {}) {}

  getHeader(name: string): string | undefined {
    return this.headers[name.toLowerCase()];
  }

  getMethod(): string {
    return "GET";
  }

  getPath(): string {
    return "/api/test";
  }

  getUrl(): string {
    return "https://example.com/api/test";
  }

  getAcceptHeader(): string {
    return "application/json";
  }

  getUserAgent(): string {
    return "Vitest";
  }
}

describe("x402HTTPResourceServer facilitator response errors", () => {
  let resourceServer: x402ResourceServer;
  let facilitator: MockFacilitatorClient;
  let httpServer: x402HTTPResourceServer;
  const network = "eip155:8453" as Network;

  beforeEach(async () => {
    facilitator = new MockFacilitatorClient(
      buildSupportedResponse({
        kinds: [{ x402Version: 2, scheme: "exact", network }],
      }),
    );

    resourceServer = new x402ResourceServer(facilitator);
    resourceServer.register(network, new MockSchemeNetworkServer("exact"));
    await resourceServer.initialize();

    httpServer = new x402HTTPResourceServer(resourceServer, {
      "/api/test": {
        accepts: {
          scheme: "exact",
          payTo: "0xabc",
          price: "$1.00",
          network,
        },
      },
    });
  });

  it("rethrows FacilitatorResponseError during verification", async () => {
    facilitator.setVerifyResponse(
      new FacilitatorResponseError("Facilitator verify returned invalid JSON: not-json"),
    );

    const accepted = buildPaymentRequirements({
      scheme: "exact",
      network,
      payTo: "0xabc",
      asset: "USDC",
      amount: "1000000",
    });
    const payload = buildPaymentPayload({
      x402Version: 2,
      accepted,
    });

    await expect(
      httpServer.processHTTPRequest({
        adapter: new MockHTTPAdapter({
          "payment-signature": encodePaymentSignatureHeader(payload),
        }),
        path: "/api/test",
        method: "GET",
        paymentHeader: encodePaymentSignatureHeader(payload),
      }),
    ).rejects.toThrow(FacilitatorResponseError);
  });

  it("rethrows FacilitatorResponseError during settlement", async () => {
    facilitator.setSettleResponse(
      new FacilitatorResponseError('Facilitator settle returned invalid data: {"success":true}'),
    );

    const accepted = buildPaymentRequirements({
      scheme: "exact",
      network,
      payTo: "0xabc",
      asset: "USDC",
      amount: "1000000",
    });
    await expect(
      httpServer.processSettlement(buildPaymentPayload({ x402Version: 2, accepted }), accepted),
    ).rejects.toThrow(FacilitatorResponseError);
  });
});

describe("RouteConfigurationError", () => {
  it("stores errors array matching constructor input", () => {
    const validationErrors: RouteValidationError[] = [
      {
        routePattern: "GET /api/weather",
        scheme: "exact",
        network: "eip155:8453",
        message: "bad route",
      },
    ];
    const error = new RouteConfigurationError(validationErrors);
    expect(error.errors).toHaveLength(1);
    expect(error.errors[0].message).toBe("bad route");
  });

  it("error.errors is RouteValidationError[] at runtime — each element has message field", () => {
    const validationErrors: RouteValidationError[] = [
      { routePattern: "GET /a", scheme: "exact", network: "eip155:8453", message: "err-1" },
      { routePattern: "POST /b", scheme: "upto", network: "eip155:1", message: "err-2" },
    ];
    const error = new RouteConfigurationError(validationErrors);
    expect(error.errors).toHaveLength(2);
    expect(error.errors[0].message).toBe("err-1");
    expect(error.errors[1].message).toBe("err-2");
  });

  it("error.name is RouteConfigurationError", () => {
    const error = new RouteConfigurationError([
      { routePattern: "GET /x", scheme: "exact", network: "eip155:8453", message: "x" },
    ]);
    expect(error.name).toBe("RouteConfigurationError");
  });

  it("error.message contains each validation error message joined with newlines", () => {
    const validationErrors: RouteValidationError[] = [
      { routePattern: "GET /a", scheme: "exact", network: "eip155:8453", message: "first error" },
      { routePattern: "GET /b", scheme: "upto", network: "eip155:1", message: "second error" },
    ];
    const error = new RouteConfigurationError(validationErrors);
    expect(error.message).toContain("first error");
    expect(error.message).toContain("second error");
  });
});
