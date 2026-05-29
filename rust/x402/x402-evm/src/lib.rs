//! EVM mechanism for the x402 payment protocol.

pub mod aggr_deferred;
pub mod constants;
pub mod exact;
pub mod types;
pub mod upto;

pub use aggr_deferred::AggrDeferredEvmScheme;
pub use exact::ExactEvmScheme;
pub use upto::UptoEvmScheme;

pub use constants::{
    PERMIT2_ADDRESS, PERMIT2_EIP712_DOMAIN_NAME, PERMIT2_EXACT_WITNESS_TYPE_STRING,
    PERMIT2_UPTO_WITNESS_TYPE_STRING, X402_EXACT_PERMIT2_PROXY_ADDRESS,
    X402_UPTO_PERMIT2_PROXY_ADDRESS,
};

pub use types::{
    is_upto_permit2_payload, AssetTransferMethod, ExactEIP3009Payload, ExactEvmPayloadV2,
    ExactPermit2Payload, Permit2Authorization, Permit2Permitted, Permit2Witness,
    UptoPermit2Authorization, UptoPermit2Payload, UptoPermit2Witness,
};
