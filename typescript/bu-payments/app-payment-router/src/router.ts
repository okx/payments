import type { UnifiedRouteConfig, UnifiedRoutesConfig } from "./types.js";

/** Compiled route entry. */
export interface CompiledRoute {
  /** `null` matches any method. */
  method: string | null;
  pattern: RegExp;
  config: UnifiedRouteConfig;
}

/**
 * Compile a route configuration map into a fast-matchable structure.
 *
 * Key syntax:
 *   "GET /api/weather"      → method=GET,  path=/api/weather
 *   "POST /api/ai/chat"     → method=POST, path=/api/ai/chat
 *   "/api/data"             → any method,  path=/api/data
 *   "GET /api/users/:id"    → method=GET,  supports `:param`
 *   "GET /api/*"            → method=GET,  prefix wildcard
 */
export function compileRoutes(routes: UnifiedRoutesConfig): CompiledRoute[] {
  return Object.entries(routes).map(([key, config]) => {
    const { method, path } = parseRouteKey(key);
    return { method, pattern: pathToRegex(path), config };
  });
}

/** Match a request against compiled routes; returns `undefined` on miss. */
export function matchRoute(
  compiled: readonly CompiledRoute[],
  path: string,
  method: string,
): UnifiedRouteConfig | undefined {
  const upper = method.toUpperCase();
  for (const route of compiled) {
    if (route.method && route.method !== upper) continue;
    if (route.pattern.test(path)) return route.config;
  }
  return undefined;
}

function parseRouteKey(key: string): { method: string | null; path: string } {
  const trimmed = key.trim();
  const idx = trimmed.indexOf(" ");
  if (idx === -1) return { method: null, path: trimmed };
  return {
    method: trimmed.slice(0, idx).toUpperCase(),
    path: trimmed.slice(idx + 1).trim(),
  };
}

function pathToRegex(path: string): RegExp {
  if (path === "*") return /^.*$/;
  const escaped = path
    .replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
    .replace(/\\:\w+/g, "[^/]+")
    .replace(/\\\*/g, ".*");
  return new RegExp(`^${escaped}$`);
}
