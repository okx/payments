"""PaymentRouter ProtocolAdapter for MPP.

Wraps an Mpp server instance and delegates detect/challenge/handle to pympp.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from mpp.server.mpp import Mpp


@dataclass
class MppRouteConfig:
    """Per-route configuration for MppAdapter.

    """

    intent: str = ""
    amount: str = ""
    currency: str = ""
    decimals: int = 6
    description: str = ""
    external_id: str = ""
    realm: str = ""
    unit_type: str = ""
    suggested_deposit: str = ""


class MppAdapter:
    """PaymentRouter ProtocolAdapter for MPP protocol.

    Wraps an Mpp server instance. Detects requests with
    'Authorization: Payment ...' header.
    """

    def __init__(self, mpp: Mpp, priority: int = 10) -> None:
        self._mpp = mpp
        self._priority = priority

    @property
    def name(self) -> str:
        return "mpp"

    @property
    def priority(self) -> int:
        return self._priority

    def detect(self, request: Any) -> bool:
        auth = request.get_header("authorization")
        if not auth:
            return False
        scheme = auth.split(" ", 1)[0]
        return scheme == "Payment"

    async def get_challenge(self, request: Any, cfg: Any) -> dict[str, str]:
        if cfg is None:
            return {}

        route_cfg = cfg if isinstance(cfg, MppRouteConfig) else MppRouteConfig(**cfg)
        if not route_cfg.intent:
            return {}

        intent_obj = self._mpp.method.intents.get(route_cfg.intent)
        if intent_obj is None:
            return {}

        from mpp.server.verify import verify_or_challenge
        from mpp.server.decorator import make_challenge_response
        from mpp import Challenge
        DEFAULT_DECIMALS = 6
        from mpp._units import parse_units

        decimals = getattr(self._mpp.method, "decimals", DEFAULT_DECIMALS)
        base_amount = str(parse_units(route_cfg.amount, decimals))
        currency = route_cfg.currency or getattr(self._mpp.method, "currency", "")
        recipient = getattr(self._mpp.method, "recipient", "")

        req: dict[str, Any] = {
            "amount": base_amount,
            "currency": currency,
            "recipient": recipient,
        }
        if route_cfg.description:
            req["description"] = route_cfg.description
        if route_cfg.unit_type:
            req["unitType"] = route_cfg.unit_type
        if route_cfg.suggested_deposit:
            req["suggestedDeposit"] = route_cfg.suggested_deposit
        if hasattr(intent_obj, "challenge_method_details"):
            req["methodDetails"] = intent_obj.challenge_method_details()

        result = await verify_or_challenge(
            authorization=None,
            intent=intent_obj,
            request=req,
            realm=self._mpp.realm,
            secret_key=self._mpp.secret_key,
            method=self._mpp.method.name,
            description=route_cfg.description or None,
        )

        if isinstance(result, Challenge):
            # Format as WWW-Authenticate header
            resp = make_challenge_response(result, self._mpp.realm)
            www_auth = resp.headers.get("www-authenticate", "")
            if www_auth:
                return {"WWW-Authenticate": www_auth}
        return {}

    async def handle(self, request: Any, cfg: Any) -> Any:
        """Verify payment credential from Authorization header.

        Delegates to Mpp's verify flow. For charge intent, returns the receipt.
        For session intent, returns the SessionVerifyResult (receipt + management response).
        """
        from mpp.server.verify import verify_or_challenge
        from mpp import Challenge

        route_cfg = cfg if isinstance(cfg, MppRouteConfig) else MppRouteConfig(**cfg)

        auth_header = request.get_header("authorization")
        if not auth_header:
            raise ValueError("missing Authorization header")

        intent_obj = self._mpp.method.intents.get(route_cfg.intent)
        if intent_obj is None:
            raise ValueError(f"unknown intent: {route_cfg.intent}")

        # Build request params matching what the decorator would build
        DEFAULT_DECIMALS = 6
        from mpp._units import parse_units

        decimals = getattr(self._mpp.method, "decimals", DEFAULT_DECIMALS)
        base_amount = str(parse_units(route_cfg.amount, decimals))
        currency = route_cfg.currency or getattr(self._mpp.method, "currency", "")
        recipient = getattr(self._mpp.method, "recipient", "")

        req: dict[str, Any] = {
            "amount": base_amount,
            "currency": currency,
            "recipient": recipient,
        }

        if route_cfg.description:
            req["description"] = route_cfg.description
        if route_cfg.unit_type:
            req["unitType"] = route_cfg.unit_type
        if route_cfg.suggested_deposit:
            req["suggestedDeposit"] = route_cfg.suggested_deposit

        if hasattr(intent_obj, "challenge_method_details"):
            req["methodDetails"] = intent_obj.challenge_method_details()

        result = await verify_or_challenge(
            authorization=auth_header,
            intent=intent_obj,
            request=req,
            realm=self._mpp.realm,
            secret_key=self._mpp.secret_key,
            method=self._mpp.method.name,
            description=route_cfg.description or None,
        )

        if isinstance(result, Challenge):
            # Should not happen since detect() already confirmed payment present
            raise ValueError("unexpected challenge during handle")

        credential, receipt = result

        # For session: check respond() for management actions
        respond_fn = getattr(intent_obj, "respond", None)
        if respond_fn is not None:
            mgmt = respond_fn(credential, receipt)
            if mgmt is not None:
                return {"_management": True, "response": mgmt}

        return receipt
