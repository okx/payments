export type {
  Handler,
  ProtocolAdapter,
  UnifiedRouteConfig,
  UnifiedRoutesConfig,
  PaymentRouterConfig,
} from "./types.js";

export { paymentRouter } from "./core.js";

export { ProtocolDetector } from "./detector.js";
export { mergeChallenges } from "./merger.js";
export { compileRoutes, matchRoute } from "./router.js";
export type { CompiledRoute } from "./router.js";

export { MppAdapter } from "./adapters/mpp.js";
export type {
  MppAdapterConfig,
  MppAdapterRouteConfig,
} from "./adapters/mpp.js";
export { X402Adapter } from "./adapters/x402.js";
export type {
  X402AdapterConfig,
  X402AdapterRouteConfig,
} from "./adapters/x402.js";
