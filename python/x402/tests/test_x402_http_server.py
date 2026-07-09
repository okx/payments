"""Unit tests for the exempt-payer signature bypass in the HTTP server."""

from __future__ import annotations

from typing import Any

from x402.http.types import (
    HTTPRequestContext,
    PaymentOption,
    RouteConfig,
)
from x402.http.utils import encode_payment_signature_header
from x402.http.x402_http_server import x402HTTPResourceServerSync
from x402.schemas import (
    PaymentPayload,
    PaymentRequired,
    PaymentRequirements,
    ResourceInfo,
    VerifyResponse,
)

PAY_TO = "0xmerchant"


class StubServer:
    """Minimal sync core server exercising the HTTP server's bypass + driver."""

    def __init__(self, sig_valid: bool = True) -> None:
        self.sig_valid = sig_valid
        self.verify_called = False
        self.verify_signature_called = False

    def build_payment_requirements(self, config: Any) -> list[PaymentRequirements]:
        return [
            PaymentRequirements(
                scheme=config.scheme,
                network=config.network,
                asset="0xUSDC",
                amount="1000000",
                pay_to=config.pay_to,
                max_timeout_seconds=config.max_timeout_seconds or 300,
            )
        ]

    def create_payment_required_response(
        self,
        requirements: list[PaymentRequirements],
        resource: ResourceInfo,
        error: Any,
        extensions: Any,
    ) -> PaymentRequired:
        return PaymentRequired(x402_version=2, accepts=requirements, resource=resource, error=error)

    def find_matching_requirements(
        self, accepts: list[PaymentRequirements], payload: PaymentPayload
    ) -> PaymentRequirements | None:
        return accepts[0] if accepts else None

    def enrich_extensions(self, extensions: Any, context: Any) -> Any:
        return extensions

    def verify_payment(self, payload: Any, requirements: Any) -> VerifyResponse:
        self.verify_called = True
        return VerifyResponse(is_valid=True, payer="0xpayer")

    def verify_signature(self, payload: Any) -> VerifyResponse:
        self.verify_signature_called = True
        return VerifyResponse(is_valid=self.sig_valid, payer="0xpayer")


class FakeAdapter:
    def __init__(self, header: str) -> None:
        self._header = header

    def get_header(self, name: str) -> str | None:
        return self._header if "payment-signature" in name.lower() else None

    def get_method(self) -> str:
        return "GET"

    def get_url(self) -> str:
        return "http://example.com/resource"

    def get_accept_header(self) -> str:
        return "application/json"

    def get_user_agent(self) -> str:
        return "test-agent"


def _routes(scheme: str = "exact") -> dict:
    return {
        "GET /resource": RouteConfig(
            accepts=[
                PaymentOption(
                    scheme=scheme,
                    price="$0.00001",
                    network="eip155:196",
                    pay_to=PAY_TO,
                    max_timeout_seconds=300,
                )
            ],
            description="paid",
            mime_type="application/json",
        )
    }


def _payload(
    from_addr: str, auth_field: str = "authorization", scheme: str = "exact"
) -> PaymentPayload:
    return PaymentPayload(
        x402_version=2,
        payload={"signature": "0xsig", auth_field: {"from": from_addr}},
        accepted=PaymentRequirements(
            scheme=scheme,
            network="eip155:196",
            asset="0xUSDC",
            amount="1000000",
            pay_to=PAY_TO,
            max_timeout_seconds=300,
        ),
    )


def _run(
    exempt: list[str] | None, payload: PaymentPayload, sig_valid: bool = True, scheme: str = "exact"
):
    stub = StubServer(sig_valid=sig_valid)
    http_server = x402HTTPResourceServerSync(stub, _routes(scheme), exempt_payers=exempt)
    header = encode_payment_signature_header(payload)
    ctx = HTTPRequestContext(adapter=FakeAdapter(header), path="/resource", method="GET")
    result = http_server.process_http_request(ctx)
    return result, stub


# An exempt payer with a valid signature is served free after verify-signature
# only, even when the balance check would otherwise fail.
def test_exempt_served_without_verify():
    result, stub = _run(["0xReviewer"], _payload("0xReviewer"))
    assert result.type == "no-payment-required"
    assert stub.verify_signature_called
    assert not stub.verify_called


# A non-exempt payer takes the normal verify path and never hits verify-signature.
def test_non_exempt_normal_path():
    result, stub = _run(["0xReviewer"], _payload("0xSomeoneElse"))
    assert result.type == "payment-verified"
    assert stub.verify_called
    assert not stub.verify_signature_called


# A listed payer with an invalid signature falls through to the paid flow; a
# forged header therefore cannot self-exempt.
def test_invalid_signature_falls_through():
    result, stub = _run(["0xReviewer"], _payload("0xReviewer"), sig_valid=False)
    assert stub.verify_signature_called
    assert stub.verify_called
    assert result.type == "payment-verified"


# An empty exempt list disables the bypass.
def test_empty_list_disables_bypass():
    result, stub = _run(None, _payload("0xReviewer"))
    assert result.type == "payment-verified"
    assert not stub.verify_signature_called


# The address match is case-insensitive.
def test_case_insensitive_match():
    result, stub = _run(["0xABCdef"], _payload("0xabcDEF"))
    assert result.type == "no-payment-required"
    assert stub.verify_signature_called


# permit2Authorization.from is matched too.
def test_permit2_authorization_matched():
    result, stub = _run(["0xReviewer"], _payload("0xReviewer", auth_field="permit2Authorization"))
    assert result.type == "no-payment-required"
    assert stub.verify_signature_called
    assert not stub.verify_called


# The aggr_deferred scheme is in scope for the bypass.
def test_aggr_deferred_scheme_bypassed():
    result, stub = _run(
        ["0xReviewer"], _payload("0xReviewer", scheme="aggr_deferred"), scheme="aggr_deferred"
    )
    assert result.type == "no-payment-required"
    assert stub.verify_signature_called
    assert not stub.verify_called


# A scheme outside the eligible set is never bypassed, even for a listed payer.
def test_ineligible_scheme_not_bypassed():
    result, stub = _run(["0xReviewer"], _payload("0xReviewer", scheme="upto"), scheme="upto")
    assert result.type == "payment-verified"
    assert not stub.verify_signature_called
    assert stub.verify_called
