"""EVM server implementation for the Aggregated Deferred payment scheme.

From the seller's perspective, the deferred scheme behaves identically to
'exact' for building payment requirements — the difference is handled
entirely by the Facilitator during verify/settle.
"""

from ....schemas import AssetAmount, Network, PaymentRequirements, Price, SupportedKind
from ..exact.server import ExactEvmScheme

SCHEME_AGGR_DEFERRED = "aggr_deferred"


class AggrDeferredEvmScheme:
    """EVM server implementation for the Aggregated Deferred payment scheme.

    Thin wrapper around ExactEvmScheme — delegates all logic since
    price parsing and requirement enhancement are identical.
    Only the scheme identifier differs.

    Attributes:
        scheme: The scheme identifier ("aggr_deferred").
    """

    scheme = SCHEME_AGGR_DEFERRED

    def __init__(self) -> None:
        self._exact = ExactEvmScheme()

    def parse_price(self, price: Price, network: Network) -> AssetAmount:
        """Parse price into asset amount. Delegates to exact scheme."""
        return self._exact.parse_price(price, network)

    def enhance_payment_requirements(
        self,
        requirements: PaymentRequirements,
        supported_kind: SupportedKind,
        extension_keys: list[str],
    ) -> PaymentRequirements:
        """Enhance payment requirements. Delegates to exact scheme."""
        return self._exact.enhance_payment_requirements(
            requirements, supported_kind, extension_keys
        )
