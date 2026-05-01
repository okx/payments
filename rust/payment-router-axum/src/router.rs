//! Route matcher. Spec §9 route matching + path normalization.
//!
//! Contract (must match other-language implementations):
//!
//! 1. Matching is first-match-wins in **declaration order**. More specific
//!    routes must be declared first; the router does NOT sort by specificity.
//! 2. Path normalization — router is the authority, not the framework:
//!    - Strip `?query` and `#fragment`
//!    - Collapse repeated slashes (`//api` → `/api`)
//!    - Strip trailing slash except for root (`/api/` → `/api`, but `/` stays `/`)
//! 3. Method normalization — uppercase. Route key without a method prefix =
//!    any method (stored as `*`).
//! 4. Route pattern syntax: `:param` matches `[^/]+`, `*` matches non-greedy `.*?`.
//!    Pattern is compiled to a regex at startup.
//!
//! Startup-time validation (`PaymentRouterLayer::new`):
//! - Every `adapter_configs` key must reference a registered adapter.name();
//!   unknown keys fail-fast with `BuildError::UnknownAdapter`.

use std::collections::HashSet;

use thiserror::Error;

use crate::types::UnifiedRouteConfig;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("route {route:?} references unknown adapter {adapter:?}; known: {known:?}")]
    UnknownAdapter {
        route: String,
        adapter: String,
        known: Vec<String>,
    },
    #[error("invalid route key {0:?}: {1}")]
    InvalidRouteKey(String, String),
    #[error("invalid route pattern {0:?}: {1}")]
    InvalidRoutePattern(String, String),
}

#[derive(Debug, Clone)]
struct CompiledRoute {
    /// Uppercase method, or `"*"` for any.
    method: String,
    /// Compiled regex. We use a tiny hand-rolled matcher (no `regex` crate
    /// dependency) because our pattern syntax is constrained: literal path,
    /// `:param` → `[^/]+`, `*` → `.*?`. Avoids pulling regex into the
    /// dependency graph for a handful of patterns.
    segments: Vec<PatternSegment>,
    /// The original route key, as declared (for error reporting and for route
    /// key lookup in downstream code).
    route_key: String,
    /// Full user config.
    config: UnifiedRouteConfig,
}

#[derive(Debug, Clone)]
enum PatternSegment {
    /// Matches a literal string exactly.
    Literal(String),
    /// Matches `[^/]+` (single path segment).
    Param,
    /// Matches `.*?` (non-greedy, any length including zero).
    Wildcard,
}

#[derive(Debug)]
pub struct CompiledRouter {
    routes: Vec<CompiledRoute>,
}

impl CompiledRouter {
    pub fn new(
        routes: Vec<(String, UnifiedRouteConfig)>,
        adapter_names: &HashSet<String>,
    ) -> Result<Self, BuildError> {
        let mut compiled = Vec::with_capacity(routes.len());
        for (raw_key, cfg) in routes {
            // validate adapter_configs keys
            for adapter_key in cfg.adapter_configs.keys() {
                if !adapter_names.contains(adapter_key) {
                    return Err(BuildError::UnknownAdapter {
                        route: raw_key.clone(),
                        adapter: adapter_key.clone(),
                        known: adapter_names.iter().cloned().collect(),
                    });
                }
            }
            let (method, path_pattern) = parse_route_key(&raw_key)?;
            let segments = compile_pattern(&path_pattern)
                .map_err(|e| BuildError::InvalidRoutePattern(raw_key.clone(), e))?;
            compiled.push(CompiledRoute {
                method,
                segments,
                route_key: raw_key,
                config: cfg,
            });
        }
        Ok(Self { routes: compiled })
    }

    /// Match a request. Returns `None` if no route matches (caller should
    /// pass-through to fallback inner service).
    pub fn match_route(&self, method: &str, path: &str) -> Option<RouteMatch<'_>> {
        let norm_method = method.to_ascii_uppercase();
        let norm_path = normalize_path(path);
        for r in &self.routes {
            if r.method != "*" && r.method != norm_method {
                continue;
            }
            if pattern_matches(&r.segments, &norm_path) {
                return Some(RouteMatch {
                    route_key: &r.route_key,
                    config: &r.config,
                });
            }
        }
        None
    }
}

pub struct RouteMatch<'a> {
    pub route_key: &'a str,
    pub config: &'a UnifiedRouteConfig,
}

// ---------------------------------------------------------------------------
// Route key parsing
// ---------------------------------------------------------------------------

/// Parses `"GET /api/foo"` → `("GET", "/api/foo")`,
/// `"/api/foo"` → `("*", "/api/foo")`.
fn parse_route_key(key: &str) -> Result<(String, String), BuildError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(BuildError::InvalidRouteKey(key.to_string(), "empty".into()));
    }
    // Method prefix must be purely alphabetic. Anything else = no method.
    let (head, rest) = match trimmed.find(char::is_whitespace) {
        Some(idx) => trimmed.split_at(idx),
        None => return Ok(("*".into(), normalize_path(trimmed))),
    };
    if head.chars().all(|c| c.is_ascii_alphabetic()) && !head.is_empty() {
        Ok((head.to_ascii_uppercase(), normalize_path(rest.trim())))
    } else {
        Ok(("*".into(), normalize_path(trimmed)))
    }
}

/// Path normalization (spec §9 route matching):
/// - strip `?query` / `#fragment`
/// - collapse `//` → `/`
/// - strip trailing `/` except root
pub(crate) fn normalize_path(path: &str) -> String {
    // strip query / fragment
    let core = path.split(['?', '#']).next().unwrap_or("");
    // collapse consecutive slashes
    let mut out = String::with_capacity(core.len());
    let mut prev_slash = false;
    for ch in core.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    // strip trailing slash (but keep root)
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        "/".to_string()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Pattern compilation + matching
// ---------------------------------------------------------------------------

fn compile_pattern(pattern: &str) -> Result<Vec<PatternSegment>, String> {
    let mut segs = Vec::new();
    let mut lit = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ':' => {
                if !lit.is_empty() {
                    segs.push(PatternSegment::Literal(std::mem::take(&mut lit)));
                }
                // consume param name
                while let Some(&nc) = chars.peek() {
                    if nc.is_alphanumeric() || nc == '_' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                segs.push(PatternSegment::Param);
            }
            '*' => {
                if !lit.is_empty() {
                    segs.push(PatternSegment::Literal(std::mem::take(&mut lit)));
                }
                segs.push(PatternSegment::Wildcard);
            }
            _ => {
                lit.push(c);
            }
        }
    }
    if !lit.is_empty() {
        segs.push(PatternSegment::Literal(lit));
    }
    Ok(segs)
}

/// Non-greedy match of `path` against `segments`.
fn pattern_matches(segments: &[PatternSegment], path: &str) -> bool {
    match_from(segments, path.as_bytes())
}

fn match_from(segments: &[PatternSegment], haystack: &[u8]) -> bool {
    if segments.is_empty() {
        return haystack.is_empty();
    }
    match &segments[0] {
        PatternSegment::Literal(lit) => {
            let lb = lit.as_bytes();
            if haystack.starts_with(lb) {
                match_from(&segments[1..], &haystack[lb.len()..])
            } else {
                false
            }
        }
        PatternSegment::Param => {
            // match a single path segment: `[^/]+`, at least 1 char
            let mut i = 0;
            while i < haystack.len() && haystack[i] != b'/' {
                i += 1;
            }
            if i == 0 {
                return false;
            }
            // try longest-first match of rest (path segments are usually non-greedy
            // per segment; `[^/]+` is greedy within a single segment)
            match_from(&segments[1..], &haystack[i..])
        }
        PatternSegment::Wildcard => {
            // non-greedy `.*?`: try smallest prefix first
            for i in 0..=haystack.len() {
                if match_from(&segments[1..], &haystack[i..]) {
                    return true;
                }
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn names() -> HashSet<String> {
        ["mpp", "x402"].into_iter().map(String::from).collect()
    }

    fn cfg_with(adapters: &[&str]) -> UnifiedRouteConfig {
        let mut map = std::collections::HashMap::new();
        for a in adapters {
            map.insert(a.to_string(), serde_json::json!({}));
        }
        UnifiedRouteConfig {
            description: None,
            adapter_configs: map,
        }
    }

    #[test]
    fn normalize_strips_query_and_fragment() {
        assert_eq!(normalize_path("/api/foo?bar=1"), "/api/foo");
        assert_eq!(normalize_path("/api/foo#x"), "/api/foo");
    }

    #[test]
    fn normalize_collapses_and_trims() {
        assert_eq!(normalize_path("//api//foo/"), "/api/foo");
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn exact_match() {
        let r = CompiledRouter::new(vec![("GET /photos".into(), cfg_with(&["mpp"]))], &names())
            .unwrap();
        assert!(r.match_route("GET", "/photos").is_some());
        assert!(r.match_route("GET", "/videos").is_none());
        assert!(r.match_route("POST", "/photos").is_none());
    }

    #[test]
    fn any_method_prefix() {
        let r =
            CompiledRouter::new(vec![("/photos".into(), cfg_with(&["mpp"]))], &names()).unwrap();
        assert!(r.match_route("GET", "/photos").is_some());
        assert!(r.match_route("POST", "/photos").is_some());
    }

    #[test]
    fn param_matches_single_segment() {
        let r = CompiledRouter::new(
            vec![("GET /users/:id".into(), cfg_with(&["mpp"]))],
            &names(),
        )
        .unwrap();
        assert!(r.match_route("GET", "/users/42").is_some());
        assert!(r.match_route("GET", "/users/42/posts").is_none());
    }

    #[test]
    fn wildcard_matches_suffix() {
        let r = CompiledRouter::new(vec![("GET /files/*".into(), cfg_with(&["mpp"]))], &names())
            .unwrap();
        assert!(r.match_route("GET", "/files/a").is_some());
        assert!(r.match_route("GET", "/files/a/b/c").is_some());
    }

    #[test]
    fn first_match_wins() {
        let r = CompiledRouter::new(
            vec![
                ("GET /api/weather".into(), cfg_with(&["mpp"])),
                ("GET /api/:anything".into(), cfg_with(&["x402"])),
            ],
            &names(),
        )
        .unwrap();
        let m = r.match_route("GET", "/api/weather").unwrap();
        assert_eq!(m.route_key, "GET /api/weather");
        assert!(m.config.adapter_configs.contains_key("mpp"));
    }

    #[test]
    fn unknown_adapter_rejected_at_build() {
        let err = CompiledRouter::new(vec![("GET /x".into(), cfg_with(&["lightning"]))], &names())
            .unwrap_err();
        match err {
            BuildError::UnknownAdapter { adapter, .. } => assert_eq!(adapter, "lightning"),
            _ => panic!("wrong error: {err:?}"),
        }
    }

    #[test]
    fn trailing_slash_match() {
        let r = CompiledRouter::new(vec![("GET /photos".into(), cfg_with(&["mpp"]))], &names())
            .unwrap();
        assert!(r.match_route("GET", "/photos/").is_some());
        assert!(r.match_route("GET", "/photos?foo=1").is_some());
    }
}
