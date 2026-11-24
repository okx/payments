/**
 * Buyer-facing route handlers. Mounted AFTER `paymentMiddleware`, so by the
 * time these run the verify/settle/access checks have already passed and
 * `req.x402` carries the resolved subscription / settle result.
 */
import type { Express, Request, Response } from "express";

/* eslint-disable @typescript-eslint/no-explicit-any */

export function mountBuyerRoutes(app: Express): void {
  const protectedHandler =
    (route: string) =>
    (req: Request, res: Response): void => {
      const x402 = (req as any).x402 ?? {};
      res.json({
        route,
        message: `🎉 access granted to ${route}`,
        subId: x402.subscription?.subId ?? x402.subId,
        planId: x402.subscription?.planId,
        planTier: x402.subscription?.planTier,
        lastChargedPeriod: x402.subscription?.lastChargedPeriod,
      });
    };
  app.get("/api/protected/basic", protectedHandler("basic"));
  app.get("/api/protected/pro", protectedHandler("pro"));
  app.get("/api/protected/enterprise", protectedHandler("enterprise"));
  app.get("/api/protected/ultimate", protectedHandler("ultimate"));

  app.get("/api/change-plan", (req: Request, res: Response) => {
    const x402 = (req as any).x402 ?? {};
    res.json({
      message: "plan change settled",
      newSubId: x402.settleResult?.data?.newSubId,
      operationType: x402.settleResult?.data?.operationType,
      scheduledFromPeriod: x402.settleResult?.data?.scheduledFromPeriod,
    });
  });

  app.post("/api/cancel-subscription", (req: Request, res: Response) => {
    const x402 = (req as any).x402 ?? {};
    res.json({
      message: "subscription canceled",
      subId: x402.subId ?? x402.settleResult?.data?.subId,
    });
  });

  app.post("/api/cancel-pending-change", (req: Request, res: Response) => {
    const x402 = (req as any).x402 ?? {};
    res.json({
      message: "pending downgrade canceled",
      subId: x402.settleResult?.data?.subId,
    });
  });
}
