package evm

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/saclient"
)

// EVMChargeMethod implements protocol.ChargeVerifier for EVM-based charge payments.
type EVMChargeMethod struct {
	chainID   uint64
	recipient string
	feePayer  bool
	saClient  saclient.SAClient
}

// NewEVMChargeMethod creates a new EVMChargeMethod with default values.
func NewEVMChargeMethod() *EVMChargeMethod {
	return &EVMChargeMethod{}
}

// WithChainID sets the expected EVM chain ID.
func (m *EVMChargeMethod) WithChainID(chainID uint64) *EVMChargeMethod {
	m.chainID = chainID
	return m
}

// WithRecipient sets the primary payment recipient address.
func (m *EVMChargeMethod) WithRecipient(recipient string) *EVMChargeMethod {
	m.recipient = recipient
	return m
}

// WithFeePayer sets whether the payee sponsors gas fees.
func (m *EVMChargeMethod) WithFeePayer(feePayer bool) *EVMChargeMethod {
	m.feePayer = feePayer
	return m
}

// WithSAClient sets the SA API client for on-chain settlement.
// If nil, the charge method operates in local-only mode.
func (m *EVMChargeMethod) WithSAClient(c saclient.SAClient) *EVMChargeMethod {
	m.saClient = c
	return m
}

// Method returns the payment method name.
func (m *EVMChargeMethod) Method() string {
	return MethodNameEVM
}

// ChallengeMethodDetails returns the EVM method details for challenge generation.
func (m *EVMChargeMethod) ChallengeMethodDetails() *EVMMethodDetails {
	fp := m.feePayer
	return &EVMMethodDetails{FeePayer: &fp}
}

// PrepareRequest applies EVM-specific transformations to the charge request before
// verification. It converts the Amount from decimal units to base units when
// Decimals is set.
func (m *EVMChargeMethod) PrepareRequest(request protocol.ChargeRequest, _ *protocol.PaymentCredential) protocol.ChargeRequest {
	prepared, err := request.WithBaseUnits()
	if err != nil {
		return request
	}
	return *prepared
}

// Verify validates the payment credential against the charge request.
func (m *EVMChargeMethod) Verify(ctx context.Context, cred *protocol.PaymentCredential, request *protocol.ChargeRequest) (*protocol.Receipt, error) {
	if cred == nil {
		return nil, protocol.VerificationErrorInvalidCredential("missing credential")
	}
	if cred.Echo == nil {
		return nil, protocol.VerificationErrorInvalidCredential("missing challenge echo")
	}
	if cred.Payload == nil {
		return nil, protocol.VerificationErrorInvalidCredential("missing payload")
	}

	challengeID := cred.Echo.ID

	switch cred.Payload.Type {
	case protocol.PayloadTypeTransaction:
		return m.handleTransaction(ctx, cred, challengeID)

	case protocol.PayloadTypeHash:
		return m.handleHash(ctx, cred, challengeID)

	case protocol.PayloadTypeProof:
		return m.handleProof(ctx, cred, challengeID)

	default:
		return nil, protocol.VerificationErrorInvalidPayload(
			fmt.Sprintf("unsupported payload type %q", cred.Payload.Type),
		)
	}
}

// handleTransaction handles charge transaction mode — parse typed payload, forward to SA.
func (m *EVMChargeMethod) handleTransaction(ctx context.Context, cred *protocol.PaymentCredential, challengeID string) (*protocol.Receipt, error) {
	if m.saClient == nil {
		return protocol.NewSuccessReceipt(challengeID, protocol.MethodName(MethodNameEVM), protocol.IntentCharge, ""), nil
	}

	var payload saclient.ChargeTransactionPayload
	if err := json.Unmarshal([]byte(cred.Payload.Payload), &payload); err != nil {
		return nil, protocol.VerificationErrorInvalidPayload(
			fmt.Sprintf("invalid charge transaction payload: %v", err),
		)
	}

	saReceipt, saErr := m.saClient.Settle(ctx, &saclient.ChargeSettleRequest{
		Challenge: cred.Echo,
		Payload:   payload,
		Source:    cred.Source,
	})
	if saErr != nil {
		return nil, protocol.VerificationErrorNetworkError(
			fmt.Sprintf("SA settle failed: %v", saErr),
		)
	}

	return protocol.NewSuccessReceipt(challengeID, protocol.MethodName(MethodNameEVM), protocol.IntentCharge, saReceipt.Reference), nil
}

// handleHash handles charge hash mode — parse typed payload, forward to SA.
func (m *EVMChargeMethod) handleHash(ctx context.Context, cred *protocol.PaymentCredential, challengeID string) (*protocol.Receipt, error) {
	if m.saClient == nil {
		return protocol.NewSuccessReceipt(challengeID, protocol.MethodName(MethodNameEVM), protocol.IntentCharge, ""), nil
	}

	var payload saclient.ChargeHashPayload
	if err := json.Unmarshal([]byte(cred.Payload.Payload), &payload); err != nil {
		return nil, protocol.VerificationErrorInvalidPayload(
			fmt.Sprintf("invalid charge hash payload: %v", err),
		)
	}

	saReceipt, saErr := m.saClient.VerifyHash(ctx, &saclient.ChargeVerifyHashRequest{
		Challenge: cred.Echo,
		Payload:   payload,
		Source:    cred.Source,
	})
	if saErr != nil {
		return nil, protocol.VerificationErrorNetworkError(
			fmt.Sprintf("SA verify hash failed: %v", saErr),
		)
	}

	return protocol.NewSuccessReceipt(challengeID, protocol.MethodName(MethodNameEVM), protocol.IntentCharge, saReceipt.Reference), nil
}

// handleProof handles proof payload — validate source/chainID, then forward to SA.
func (m *EVMChargeMethod) handleProof(ctx context.Context, cred *protocol.PaymentCredential, challengeID string) (*protocol.Receipt, error) {
	if cred.Source == "" {
		return nil, protocol.VerificationErrorInvalidCredential("missing source for proof payload")
	}

	parsed, err := parseProofSource(cred.Source)
	if err != nil {
		return nil, protocol.VerificationErrorInvalidCredential(
			fmt.Sprintf("invalid proof source: %v", err),
		)
	}

	if m.chainID != 0 && parsed.ChainID != m.chainID {
		return nil, protocol.VerificationErrorChainIdMismatch(
			fmt.Sprintf("expected chain %d, got %d", m.chainID, parsed.ChainID),
		)
	}

	var settlement string
	if m.saClient != nil {
		var payload saclient.ChargeTransactionPayload
		if err := json.Unmarshal([]byte(cred.Payload.Payload), &payload); err == nil {
			saReceipt, saErr := m.saClient.Settle(ctx, &saclient.ChargeSettleRequest{
				Challenge: cred.Echo,
				Payload:   payload,
				Source:    cred.Source,
			})
			if saErr != nil {
				return nil, protocol.VerificationErrorNetworkError(
					fmt.Sprintf("SA settle failed: %v", saErr),
				)
			}
			settlement = saReceipt.Reference
		}
	}

	return protocol.NewSuccessReceipt(challengeID, protocol.MethodName(MethodNameEVM), protocol.IntentCharge, settlement), nil
}
