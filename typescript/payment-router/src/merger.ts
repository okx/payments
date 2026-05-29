import type { ProtocolAdapter, UnifiedRouteConfig } from "./types.js";

/**
 * Concurrently collect challenge headers from every eligible adapter and
 * merge them into a single header map.
 *
 * - Only calls adapters that have an explicit entry in
 *   `routeConfig.adapterConfigs`.
 * - An adapter that throws is reported via `onError` and skipped; other
 *   adapters are unaffected.
 * - `WWW-Authenticate` values are concatenated with `", "`.
 * - For any other duplicate header, the later value wins.
 */
export async function mergeChallenges(
  request: Request,
  routeConfig: UnifiedRouteConfig,
  adapters: readonly ProtocolAdapter[],
  onError?: (error: unknown, protocol: string) => void,
): Promise<Record<string, string>> {
  const eligible = adapters.filter((a) => has(routeConfig, a.name));

  const results = await Promise.allSettled(
    eligible.map((a) =>
      a.buildChallenge(request, routeConfig.adapterConfigs[a.name]),
    ),
  );

  const merged: Record<string, string> = {};

  for (let i = 0; i < eligible.length; i++) {
    const r = results[i];
    const a = eligible[i];

    if (r.status === "rejected") {
      onError?.(r.reason, a.name);
      continue;
    }

    for (const [k, v] of Object.entries(r.value)) {
      if (k.toLowerCase() === "www-authenticate") {
        const existing = Object.keys(merged).find(
          (kk) => kk.toLowerCase() === "www-authenticate",
        );
        if (existing) {
          merged[existing] = `${merged[existing]}, ${v}`;
        } else {
          merged[k] = v;
        }
      } else {
        merged[k] = v;
      }
    }
  }

  return merged;
}

function has(cfg: UnifiedRouteConfig, name: string): boolean {
  return (
    cfg.adapterConfigs != null &&
    name in cfg.adapterConfigs &&
    cfg.adapterConfigs[name] != null
  );
}
