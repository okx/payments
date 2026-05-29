package evm

import (
	"fmt"
	"math/big"
)

// Payment method constants.
const DefaultExpiresMinutes = 5
const MethodNameEVM = "evm"
const MaxSplits = 10

// Default EIP-712 domain constants for voucher signing.
const DefaultDomainName = "EVM Payment Channel"
const DefaultDomainVersion = "1"

// DefaultEscrowContract is the default escrow contract address on X Layer.
const DefaultEscrowContract = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b"

// EIP-712 domain constants for proof signing.
const ProofDomainName = "MPP"
const ProofDomainVersion = "1"

// XLayerChainID is the chain ID for the X Layer network.
const XLayerChainID uint64 = 196

// Session action type constants.
const (
	ActionOpen    = "open"
	ActionTopUp   = "topUp"
	ActionVoucher = "voucher"
	ActionClose   = "close"
	ActionSettle  = "settle"
)

// Session receipt status constants.
const (
	StatusOpen   = "open"
	StatusClosed = "closed"
)

// Split describes a secondary recipient portion of a payment.
type Split struct {
	Amount    string  `json:"amount"`
	Memo      *string `json:"memo,omitempty"`
	Recipient string  `json:"recipient"`
}

// Transfer is the resolved, on-chain form of a payment leg ready for submission.
type Transfer struct {
	Amount    *big.Int
	Recipient string
	Memo      *[32]byte
}

// EVMMethodDetails holds the EVM-specific fields in a charge request's method_details.
type EVMMethodDetails struct {
	ChainID  *uint64 `json:"chainId,omitempty"`
	FeePayer *bool   `json:"feePayer,omitempty"`
	Memo     *string `json:"memo,omitempty"`
	Splits   []Split `json:"splits,omitempty"`
}

// IsFeePayer reports whether the fee-payer flag is set. Returns false when absent.
func (d *EVMMethodDetails) IsFeePayer() bool {
	if d.FeePayer == nil {
		return false
	}
	return *d.FeePayer
}

// SessionSplit describes a bps-based secondary recipient portion of a session payment.
type SessionSplit struct {
	Recipient string  `json:"recipient"`
	Bps       uint32  `json:"bps"`
	Memo      *string `json:"memo,omitempty"`
}

// EVMSessionMethodDetails holds the EVM-specific fields in a session request's method_details.
type EVMSessionMethodDetails struct {
	EscrowContract  string         `json:"escrowContract"`
	ChannelID       *string        `json:"channelId,omitempty"`
	MinVoucherDelta *string        `json:"minVoucherDelta,omitempty"`
	ChainID         *uint64        `json:"chainId,omitempty"`
	FeePayer        *bool          `json:"feePayer,omitempty"`
	Splits          []SessionSplit `json:"splits,omitempty"`
}

// IsFeePayer reports whether the fee-payer flag is set. Returns false when absent.
func (d *EVMSessionMethodDetails) IsFeePayer() bool {
	if d.FeePayer == nil {
		return false
	}
	return *d.FeePayer
}

// Payload is the base type embedded by all session credential payloads.
type Payload struct {
	Action string `json:"action"`
}

// OpenAuthorization holds the EIP-3009 authorization fields from the CLI payload.
type OpenAuthorization struct {
	From  string `json:"from"`
	Value string `json:"value"`
}

// OpenPayload is the credential payload for opening a payment channel.
type OpenPayload struct {
	Payload
	// Type is "transaction" (server broadcasts via relayer) or "hash" (client already broadcast).
	Type string `json:"type"`
	// ChannelID is the computed channel identifier (bytes32 hex, client pre-computed).
	ChannelID string `json:"channelId"`
	// Salt is the random bytes32 used to compute channelId.
	Salt string `json:"salt"`
	// CumulativeAmount is the initial voucher cumulative amount (typically "0").
	CumulativeAmount string `json:"cumulativeAmount"`
	// Signature is REQUIRED in both modes:
	//   - transaction mode: EIP-3009 deposit authorization signature
	//   - hash mode: EIP-712 voucher signature for the initial amount
	Signature string `json:"signature"`
	// Authorization carries the EIP-3009 auth (transaction mode only).
	Authorization *OpenAuthorization `json:"authorization,omitempty"`
	// Hash is the on-chain open tx hash (hash mode only, feePayer=false).
	Hash string `json:"hash,omitempty"`
	// VoucherSignature is the EIP-712 voucher signature for the initial amount
	// (transaction mode field name per spec).
	VoucherSignature string `json:"voucherSignature,omitempty"`
	// AuthorizedSigner is the address delegated to sign vouchers (defaults to payer).
	AuthorizedSigner string `json:"authorizedSigner,omitempty"`
	// Deposit is the initial deposit amount (legacy field, prefer Authorization.Value).
	Deposit string `json:"deposit,omitempty"`
}

// Validate checks that all required fields are present per spec.
func (p *OpenPayload) Validate() error {
	if p.Type == "" {
		return fmt.Errorf("open: missing required field \"type\"")
	}
	if p.Type != "transaction" && p.Type != "hash" {
		return fmt.Errorf("open: invalid type %q (want \"transaction\" or \"hash\")", p.Type)
	}
	if p.ChannelID == "" {
		return fmt.Errorf("open: missing required field \"channelId\"")
	}
	if p.Salt == "" {
		return fmt.Errorf("open: missing required field \"salt\"")
	}
	if p.CumulativeAmount == "" {
		return fmt.Errorf("open: missing required field \"cumulativeAmount\"")
	}
	if p.Signature == "" {
		return fmt.Errorf("open: missing required field \"signature\"")
	}
	if p.Type != "hash" && p.EffectiveDeposit() == "" {
		return fmt.Errorf("open: missing deposit (set deposit or authorization.value)")
	}
	if p.Type == "hash" && p.Hash == "" {
		return fmt.Errorf("open: hash mode requires \"hash\" field")
	}
	if p.Type == "transaction" {
		if p.Authorization == nil {
			return fmt.Errorf("open: transaction mode requires \"authorization\"")
		}
		if p.Authorization.From == "" {
			return fmt.Errorf("open: transaction mode requires \"authorization.from\"")
		}
		if p.VoucherSignature == "" {
			return fmt.Errorf("open: transaction mode requires \"voucherSignature\"")
		}
	}
	return nil
}

// EffectiveDeposit returns the deposit amount, checking Deposit first,
// then falling back to Authorization.Value.
func (p *OpenPayload) EffectiveDeposit() string {
	if p.Deposit != "" {
		return p.Deposit
	}
	if p.Authorization != nil {
		return p.Authorization.Value
	}
	return ""
}

// TopUpPayload is the credential payload for topping up an existing channel.
type TopUpPayload struct {
	Payload
	// Type is "transaction" (server broadcasts) or "hash" (client already broadcast).
	Type string `json:"type"`
	// ChannelID identifies the channel to top up.
	ChannelID string `json:"channelId"`
	// AdditionalDeposit is the additional deposit amount as a decimal string.
	AdditionalDeposit string `json:"additionalDeposit"`
	// Authorization carries the EIP-3009 auth (transaction mode only).
	Authorization *OpenAuthorization `json:"authorization,omitempty"`
	// Hash is the on-chain topUp tx hash (hash mode only, feePayer=false).
	Hash string `json:"hash,omitempty"`
	// Signature is the EIP-3009 signature (transaction mode).
	Signature string `json:"signature,omitempty"`
	// TopUpSalt is the salt for topUp nonce derivation (transaction mode).
	TopUpSalt string `json:"topUpSalt,omitempty"`
}

// Validate checks that all required fields are present per spec.
func (p *TopUpPayload) Validate() error {
	if p.Type == "" {
		return fmt.Errorf("topup: missing required field \"type\"")
	}
	if p.Type != "transaction" && p.Type != "hash" {
		return fmt.Errorf("topup: invalid type %q (want \"transaction\" or \"hash\")", p.Type)
	}
	if p.ChannelID == "" {
		return fmt.Errorf("topup: missing required field \"channelId\"")
	}
	if p.AdditionalDeposit == "" {
		return fmt.Errorf("topup: missing required field \"additionalDeposit\"")
	}
	if p.Type == "hash" && p.Hash == "" {
		return fmt.Errorf("topup: hash mode requires \"hash\" field")
	}
	if p.Type == "transaction" {
		if p.Authorization == nil {
			return fmt.Errorf("topup: transaction mode requires \"authorization\"")
		}
		if p.Signature == "" {
			return fmt.Errorf("topup: transaction mode requires \"signature\"")
		}
		if p.TopUpSalt == "" {
			return fmt.Errorf("topup: transaction mode requires \"topUpSalt\"")
		}
	}
	return nil
}

// VoucherPayload is the credential payload for an incremental payment voucher.
type VoucherPayload struct {
	Payload
	// ChannelID identifies the channel this voucher belongs to.
	ChannelID string `json:"channelId"`
	// CumulativeAmount is the cumulative amount authorized so far, as a decimal string.
	CumulativeAmount string `json:"cumulativeAmount"`
	// Signature is the payer's EIP-712 signature over the voucher.
	Signature string `json:"signature"`
}

// Validate checks that all required fields are present per spec.
func (p *VoucherPayload) Validate() error {
	if p.ChannelID == "" {
		return fmt.Errorf("voucher: missing required field \"channelId\"")
	}
	if p.CumulativeAmount == "" {
		return fmt.Errorf("voucher: missing required field \"cumulativeAmount\"")
	}
	if p.Signature == "" {
		return fmt.Errorf("voucher: missing required field \"signature\"")
	}
	return nil
}

// ClosePayload is the credential payload for closing a payment channel.
type ClosePayload struct {
	Payload
	// ChannelID identifies the channel to close.
	ChannelID string `json:"channelId"`
	// CumulativeAmount is the final cumulative amount, as a decimal string.
	CumulativeAmount string `json:"cumulativeAmount"`
	// Signature is the payer's EIP-712 signature over the close message.
	Signature string `json:"signature"`
}

// Validate checks that all required fields are present per spec.
func (p *ClosePayload) Validate() error {
	if p.ChannelID == "" {
		return fmt.Errorf("close: missing required field \"channelId\"")
	}
	if p.CumulativeAmount == "" {
		return fmt.Errorf("close: missing required field \"cumulativeAmount\"")
	}
	if p.Signature == "" {
		return fmt.Errorf("close: missing required field \"signature\"")
	}
	return nil
}
