/**
 * Standalone seller admin entrypoint — just boots the seller server and
 * leaves it running, no buyer driver. Visit http://127.0.0.1:4242/admin to
 * watch subscriptions, charge, and cancel.
 *
 *   pnpm seller
 */
/* eslint-disable no-console */
import { startSeller } from "./seller";

async function main(): Promise<void> {
  const port = Number(process.env.SELLER_PORT ?? 4242);
  const seller = await startSeller(port);

  const shutdown = async (sig: string) => {
    console.log(`\n[seller-admin] received ${sig}, shutting down`);
    await seller.close();
    process.exit(0);
  };
  process.on("SIGINT", () => void shutdown("SIGINT"));
  process.on("SIGTERM", () => void shutdown("SIGTERM"));
}

void main();
