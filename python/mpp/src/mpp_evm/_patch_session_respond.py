"""Monkey-patch pympp to support intent.respond() in Mpp.pay().

Temporary shim for https://github.com/tempoxyz/pympp/issues/139.
DELETE THIS FILE once pympp ships native respond hook support.

How it works:
  - Replaces ``mpp.server.decorator.wrap_payment_handler`` with a version
    that accepts an optional ``respond_fn`` parameter.
  - Replaces ``mpp.server.mpp.Mpp.pay`` to pass ``respond_fn`` when the
    intent object has a ``respond`` method (e.g., SessionIntent).
  - All existing charge flows are unaffected (respond_fn defaults to None).

Import this module once at startup (done automatically via mpp_evm/__init__.py).
"""

from __future__ import annotations

import inspect
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime, timedelta
from functools import wraps
from typing import Any, TypeVar

from mpp import Challenge, Credential, Receipt
from mpp._units import parse_units
from mpp.server.decorator import get_authorization, make_challenge_response
from mpp.server.method import transform_request
from mpp.server.verify import verify_or_challenge

R = TypeVar("R")
DEFAULT_DECIMALS = 6

# ---------------------------------------------------------------------------
# Patched wrap_payment_handler — adds optional respond_fn
# ---------------------------------------------------------------------------


def _wrap_payment_handler_with_respond(
    handler: Callable[..., Awaitable[R]],
    verify_fn: Callable[[str | None, Any], Awaitable[Challenge | tuple[Credential, Receipt]]],
    realm_fn: Callable[[], str],
    respond_fn: Callable[[Credential, Any], Any] | None = None,
) -> Callable[..., Awaitable[R | Any]]:
    """``wrap_payment_handler`` + respond hook for session management actions.

    Identical to pympp's ``wrap_payment_handler`` except:
      - Accepts optional ``respond_fn(credential, receipt) -> response | None``.
      - When ``respond_fn`` returns non-None, short-circuits with that response
        (management action). When None, proceeds to handler (voucher action).
    """
    sig = inspect.signature(handler)
    params = [p for name, p in sig.parameters.items() if name not in ("credential", "receipt")]
    new_sig = sig.replace(parameters=params)

    request_param_name = params[0].name if params else "request"

    @wraps(handler)
    async def wrapper(*args: Any, **kwargs: Any) -> R | Any:
        if args:
            request_obj = args[0]
        else:
            request_obj = kwargs.get(request_param_name)

        if request_obj is None:
            raise TypeError(
                f"Missing request argument '{request_param_name}'. "
                "The decorated handler must receive a request object as its first argument."
            )

        authorization = get_authorization(request_obj)

        try:
            result = await verify_fn(authorization, request_obj)
        except Exception as exc:
            status = getattr(exc, "status_code", None)
            if status is not None:
                try:
                    from starlette.responses import JSONResponse
                    if hasattr(exc, "to_problem_details"):
                        return JSONResponse(status_code=status, content=exc.to_problem_details())
                    return JSONResponse(status_code=status, content={"error": str(exc)})
                except ImportError:
                    pass
            raise

        if isinstance(result, Challenge):
            return make_challenge_response(result, realm_fn())

        credential, receipt = result

        # -- BEGIN respond hook (pympp#139 shim) --
        if respond_fn is not None:
            mgmt = respond_fn(credential, receipt)
            if mgmt is not None:
                try:
                    from starlette.responses import JSONResponse

                    return JSONResponse(status_code=200, content=mgmt)
                except ImportError:
                    return mgmt
        # -- END respond hook --

        return await handler(request_obj, credential, receipt)

    wrapper.__signature__ = new_sig  # type: ignore[attr-defined]

    return wrapper


# ---------------------------------------------------------------------------
# Patched Mpp.pay — threads respond_fn from intent
# ---------------------------------------------------------------------------


def _patched_pay(
    self: Any,
    amount: str,
    *,
    intent: str = "charge",
    currency: str | None = None,
    recipient: str | None = None,
    description: str | None = None,
    expires_in: timedelta | None = None,
    chain_id: int | None = None,
    extra: dict[str, str] | None = None,
    unit_type: str | None = None,
    suggested_deposit: str | None = None,
) -> Callable[
    [Callable[[Any, Credential, Receipt], Awaitable[R]]],
    Callable[[Any], Awaitable[R | Any]],
]:
    """Mpp.pay() replacement that passes respond_fn for session intents.

    Identical to pympp's ``Mpp.pay()`` except line that calls
    ``wrap_payment_handler`` also passes ``respond_fn`` when the intent
    has a ``respond`` method.
    """
    intent_obj = self.method.intents.get(intent)
    if intent_obj is None:
        raise ValueError(f"Method {self.method.name} does not support {intent} intent")

    respond_fn = getattr(intent_obj, "respond", None)

    def decorator(
        handler: Callable[[Any, Credential, Receipt], Awaitable[R]],
    ) -> Callable[[Any], Awaitable[R | Any]]:
        async def _verify(
            authorization: str | None, _request_obj: Any
        ) -> Challenge | tuple[Credential, Receipt]:
            resolved_currency = currency or getattr(self.method, "currency", None)
            resolved_recipient = recipient or getattr(self.method, "recipient", None)
            if not resolved_currency:
                raise ValueError("currency must be set on the method or passed to pay()")
            if not resolved_recipient:
                raise ValueError("recipient must be set on the method or passed to pay()")

            decimals = getattr(self.method, "decimals", DEFAULT_DECIMALS)
            base_amount = str(parse_units(amount, decimals))

            challenge_expires: str | None = None
            if expires_in is not None:
                challenge_expires = (datetime.now(UTC) + expires_in).isoformat()

            request: dict[str, Any] = {
                "amount": base_amount,
                "currency": resolved_currency,
                "recipient": resolved_recipient,
            }

            if extra is not None:
                if any(
                    not isinstance(k, str) or not isinstance(v, str) for k, v in extra.items()
                ):
                    raise ValueError("extra must be a dict[str, str]")
                request["extra"] = extra

            # Use intent's challenge_method_details() if available (includes feePayer etc),
            # otherwise fall back to basic chainId.
            if hasattr(intent_obj, "challenge_method_details"):
                request["methodDetails"] = intent_obj.challenge_method_details()
            else:
                resolved_chain_id = chain_id
                if resolved_chain_id is None:
                    resolved_chain_id = getattr(self.method, "chain_id", None)
                if resolved_chain_id is not None:
                    request["methodDetails"] = {"chainId": resolved_chain_id}

            if description is not None:
                request["description"] = description
            if unit_type is not None:
                request["unitType"] = unit_type
            if suggested_deposit is not None:
                request["suggestedDeposit"] = suggested_deposit

            request = transform_request(
                self.method,
                request,
                None,
            )

            return await verify_or_challenge(
                authorization=authorization,
                intent=intent_obj,
                request=request,
                realm=self.realm,
                secret_key=self.secret_key,
                method=self.method.name,
                description=description,
                expires=challenge_expires,
            )

        return _wrap_payment_handler_with_respond(
            handler, _verify, lambda: self.realm, respond_fn
        )

    return decorator


# ---------------------------------------------------------------------------
# Apply patches
# ---------------------------------------------------------------------------


def _apply() -> None:
    """Patch pympp's Mpp.pay and wrap_payment_handler. Idempotent."""
    import mpp.server.decorator as _dec_mod
    import mpp.server.mpp as _mpp_mod

    if getattr(_mpp_mod.Mpp.pay, "_session_respond_patched", False):
        return  # already patched

    _dec_mod.wrap_payment_handler = _wrap_payment_handler_with_respond
    _mpp_mod.Mpp.pay = _patched_pay
    _mpp_mod.Mpp.pay._session_respond_patched = True  # type: ignore[attr-defined]


_apply()
