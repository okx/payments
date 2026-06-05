"""PaymentRouter — unified multi-protocol payment middleware for Python web frameworks."""

from paymentrouter.adapter import ProtocolAdapter, RequestAdapter
from paymentrouter.detector import detect
from paymentrouter.fastapi.middleware import PaymentGate, PaymentMiddleware
from paymentrouter.merger import merge_challenges
from paymentrouter.router import Router
from paymentrouter.types import RouteConfig, RouteEntry

__all__ = [
    "PaymentGate",
    "PaymentMiddleware",
    "ProtocolAdapter",
    "RequestAdapter",
    "RouteConfig",
    "RouteEntry",
    "Router",
    "detect",
    "merge_challenges",
]
