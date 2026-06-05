"""EIP-712 voucher signature verification and signing.

- Voucher type hash and domain separator construction
- verify_voucher: ecrecover-based signature verification
- sign_voucher: sign a voucher using a Signer
- sign_authorization: sign SettleAuthorization / CloseAuthorization
"""

from __future__ import annotations

from eth_abi import encode as abi_encode
from eth_utils import keccak
from eth_keys import keys as eth_keys

from mpp_evm.errors import InvalidSignatureError
from mpp_evm.signer import Signer


# secp256k1 half-order for low-s validation (malleability prevention)
SECP256K1_HALF_N = 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0

# EIP-712 type hashes (precomputed keccak256 of type strings)
EIP712_DOMAIN_TYPEHASH = keccak(
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
)

VOUCHER_TYPEHASH = keccak(
    b"Voucher(bytes32 channelId,uint128 cumulativeAmount)"
)

SETTLE_AUTH_TYPEHASH = keccak(
    b"SettleAuthorization(bytes32 channelId,uint128 cumulativeAmount,uint256 nonce,uint256 deadline)"
)

CLOSE_AUTH_TYPEHASH = keccak(
    b"CloseAuthorization(bytes32 channelId,uint128 cumulativeAmount,uint256 nonce,uint256 deadline)"
)

DEFAULT_DOMAIN_NAME = "EVM Payment Channel"
DEFAULT_DOMAIN_VERSION = "1"


def _to_address_bytes(addr: str) -> bytes:
    """Convert hex address string to 20 bytes."""
    return bytes.fromhex(addr.removeprefix("0x").lower().zfill(40))


def compute_domain_separator(
    name: str,
    version: str,
    chain_id: int,
    verifying_contract: str,
) -> bytes:
    """Compute EIP-712 domain separator hash."""
    return keccak(
        abi_encode(
            ["bytes32", "bytes32", "bytes32", "uint256", "address"],
            [
                EIP712_DOMAIN_TYPEHASH,
                keccak(name.encode()),
                keccak(version.encode()),
                chain_id,
                _to_address_bytes(verifying_contract),
            ],
        )
    )


def compute_voucher_struct_hash(channel_id: bytes, cumulative_amount: int) -> bytes:
    """Compute EIP-712 struct hash for Voucher(channelId, cumulativeAmount)."""
    return keccak(
        abi_encode(
            ["bytes32", "bytes32", "uint128"],
            [VOUCHER_TYPEHASH, channel_id, cumulative_amount],
        )
    )


def compute_signing_hash(domain_separator: bytes, struct_hash: bytes) -> bytes:
    """Compute EIP-712 signing hash: keccak256(0x1901 || domainSep || structHash)."""
    return keccak(b"\x19\x01" + domain_separator + struct_hash)


def voucher_signing_hash(
    channel_id: bytes,
    cumulative_amount: int,
    escrow_contract: str,
    chain_id: int,
    domain_name: str = "",
    domain_version: str = "",
) -> bytes:
    """Compute the EIP-712 signing hash for a voucher.

    """
    name = domain_name or DEFAULT_DOMAIN_NAME
    version = domain_version or DEFAULT_DOMAIN_VERSION
    domain_sep = compute_domain_separator(name, version, chain_id, escrow_contract)
    struct_hash = compute_voucher_struct_hash(channel_id, cumulative_amount)
    return compute_signing_hash(domain_sep, struct_hash)


def validate_voucher_signature(sig: bytes) -> None:
    """Validate voucher signature format: 65 bytes, low-s.

    Raises InvalidSignatureError on failure.
    """
    if len(sig) != 65:
        raise InvalidSignatureError(detail=f"signature must be 65 bytes, got {len(sig)}")
    s_value = int.from_bytes(sig[32:64], "big")
    if s_value > SECP256K1_HALF_N:
        raise InvalidSignatureError(detail="signature has high-s value (malleable)")


def verify_voucher(
    *,
    escrow_contract: str,
    chain_id: int,
    channel_id: bytes,
    cumulative_amount: int,
    signature: bytes,
    expected_signer: str,
    domain_name: str = "",
    domain_version: str = "",
) -> bool:
    """Verify a voucher signature via ecrecover.

    Returns True if recovered address matches expected_signer.
    """
    msg_hash = voucher_signing_hash(
        channel_id, cumulative_amount, escrow_contract, chain_id, domain_name, domain_version
    )

    sig_copy = bytearray(signature)
    if sig_copy[64] >= 27:
        sig_copy[64] -= 27

    try:
        sig = eth_keys.Signature(signature_bytes=bytes(sig_copy))
        recovered = sig.recover_public_key_from_msg_hash(bytes(msg_hash)).to_checksum_address()
    except Exception:
        return False

    return recovered.lower() == expected_signer.lower()


def verify_voucher_or_raise(
    *,
    escrow_contract: str,
    chain_id: int,
    channel_id: bytes,
    cumulative_amount: int,
    signature: bytes,
    expected_signer: str,
    domain_name: str = "",
    domain_version: str = "",
) -> None:
    """Verify a voucher signature, raising InvalidSignatureError on failure."""
    if not verify_voucher(
        escrow_contract=escrow_contract,
        chain_id=chain_id,
        channel_id=channel_id,
        cumulative_amount=cumulative_amount,
        signature=signature,
        expected_signer=expected_signer,
        domain_name=domain_name,
        domain_version=domain_version,
    ):
        raise InvalidSignatureError(detail="voucher signature verification failed")


def sign_voucher(
    *,
    signer: Signer,
    channel_id: bytes,
    cumulative_amount: int,
    escrow_contract: str,
    chain_id: int,
    domain_name: str = "",
    domain_version: str = "",
) -> bytes:
    """Sign a voucher using the provided Signer.

    Returns 65-byte signature.
    """
    msg_hash = voucher_signing_hash(
        channel_id, cumulative_amount, escrow_contract, chain_id, domain_name, domain_version
    )
    return signer.sign(msg_hash)


def _authorization_signing_hash(
    primary_type: str,
    channel_id: bytes,
    cumulative_amount: int,
    nonce: int,
    deadline: int,
    escrow_contract: str,
    chain_id: int,
    domain_name: str = "",
    domain_version: str = "",
) -> bytes:
    """Compute EIP-712 signing hash for SettleAuthorization or CloseAuthorization."""
    name = domain_name or DEFAULT_DOMAIN_NAME
    version = domain_version or DEFAULT_DOMAIN_VERSION

    if primary_type == "SettleAuthorization":
        typehash = SETTLE_AUTH_TYPEHASH
    elif primary_type == "CloseAuthorization":
        typehash = CLOSE_AUTH_TYPEHASH
    else:
        raise ValueError(f"invalid authorization type: {primary_type!r}")

    domain_sep = compute_domain_separator(name, version, chain_id, escrow_contract)
    struct_hash = keccak(
        abi_encode(
            ["bytes32", "bytes32", "uint128", "uint256", "uint256"],
            [typehash, channel_id, cumulative_amount, nonce, deadline],
        )
    )
    return compute_signing_hash(domain_sep, struct_hash)


def sign_authorization(
    *,
    signer: Signer,
    primary_type: str,
    channel_id: bytes,
    cumulative_amount: int,
    nonce: int,
    deadline: int,
    escrow_contract: str,
    chain_id: int,
    domain_name: str = "",
    domain_version: str = "",
) -> bytes:
    """Sign a SettleAuthorization or CloseAuthorization.

    Returns 65-byte signature.
    """
    msg_hash = _authorization_signing_hash(
        primary_type, channel_id, cumulative_amount, nonce, deadline,
        escrow_contract, chain_id, domain_name, domain_version,
    )
    return signer.sign(msg_hash)
