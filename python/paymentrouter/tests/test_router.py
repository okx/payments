"""Tests for paymentrouter.router — compiled regex route matching."""

from __future__ import annotations

from paymentrouter.router import Router


class TestRouter:
    def test_exact_method_path_match(self) -> None:
        """'GET /resource' matches GET /resource."""
        router = Router({"GET /resource": {"adapter": "cfg"}})

        result = router.match("GET", "/resource")

        assert result is not None
        cfg, pattern = result
        assert cfg == {"adapter": "cfg"}
        assert pattern == "GET /resource"

    def test_any_method_match(self) -> None:
        """'/resource' matches GET and POST /resource."""
        router = Router({"/resource": {"adapter": "cfg"}})

        assert router.match("GET", "/resource") is not None
        assert router.match("POST", "/resource") is not None
        assert router.match("DELETE", "/resource") is not None

    def test_wildcard_match(self) -> None:
        """'*' matches any method and path."""
        router = Router({"*": {"adapter": "cfg"}})

        assert router.match("GET", "/anything") is not None
        assert router.match("POST", "/foo/bar") is not None

    def test_param_match(self) -> None:
        """'GET /users/:id' matches GET /users/123."""
        router = Router({"GET /users/:id": {"adapter": "cfg"}})

        result = router.match("GET", "/users/123")

        assert result is not None

    def test_wildcard_in_path(self) -> None:
        """'GET /api/*' matches GET /api/foo/bar."""
        router = Router({"GET /api/*": {"adapter": "cfg"}})

        result = router.match("GET", "/api/foo/bar")

        assert result is not None

    def test_no_match_returns_none(self) -> None:
        """Unregistered path returns None."""
        router = Router({"GET /resource": {"adapter": "cfg"}})

        assert router.match("GET", "/other") is None
        assert router.match("POST", "/resource") is None

    def test_first_match_wins(self) -> None:
        """Overlapping patterns, first registered wins."""
        router = Router({
            "GET /api/weather": {"winner": "first"},
            "GET /api/*": {"winner": "second"},
        })

        result = router.match("GET", "/api/weather")

        assert result is not None
        cfg, _ = result
        assert cfg["winner"] == "first"

    def test_trailing_slash_normalization(self) -> None:
        """/resource/ matches /resource."""
        router = Router({"GET /resource": {"adapter": "cfg"}})

        result = router.match("GET", "/resource/")

        assert result is not None

    def test_param_no_slash_cross(self) -> None:
        """:param should not match across slash boundaries."""
        router = Router({"GET /users/:id": {"adapter": "cfg"}})

        assert router.match("GET", "/users/42/extra") is None

    def test_query_string_normalization(self) -> None:
        """Query string is stripped before matching."""
        router = Router({"GET /api/v1": {"adapter": "cfg"}})

        result = router.match("GET", "/api/v1?foo=bar")

        assert result is not None

    def test_double_slash_normalization(self) -> None:
        """Double slashes are collapsed."""
        router = Router({"GET /api/v1": {"adapter": "cfg"}})

        result = router.match("GET", "/api//v1")

        assert result is not None

    def test_fragment_normalization(self) -> None:
        """Fragment is stripped before matching."""
        router = Router({"GET /api/v1": {"adapter": "cfg"}})

        result = router.match("GET", "/api/v1#section")

        assert result is not None

    def test_empty_path_defaults_to_root(self) -> None:
        """Empty path normalizes to /."""
        router = Router({"GET /": {"adapter": "cfg"}})

        result = router.match("GET", "")

        assert result is not None
