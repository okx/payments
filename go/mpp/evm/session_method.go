package evm

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math/big"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/saclient"
	"github.com/okx/payments/go/mpp/store"
)

// proofSource holds the components extracted from a DID PKH proof source string.
type proofSource struct {
	Address string
	ChainID uint64
}

// parseProofSource parses a DID PKH source string of the form
// "did:pkh:eip155:{chainID}:{address}" and returns the decoded components.
func parseProofSource(source string) (*proofSource, error) {
	const prefix = "did:pkh:eip155:"
	if !strings.HasPrefix(source, prefix) {
		return nil, fmt.Errorf("proof source: expected prefix %q, got %q", prefix, source)
	}
	rest := source[len(prefix):]

	colonIdx := strings.Index(rest, ":")
	if colonIdx < 0 {
		return nil, fmt.Errorf("proof source: missing chainId/address separator in %q", source)
	}

	chainIDStr := rest[:colonIdx]
	addrStr := rest[colonIdx+1:]

	chainID, err := strconv.ParseUint(chainIDStr, 10, 64)
	if err != nil {
		return nil, fmt.Errorf("proof source: invalid chainId %q: %w", chainIDStr, err)
	}

	addr, err := ParseAddress(addrStr)
	if err != nil {
		return nil, fmt.Errorf("proof source: invalid address %q: %w", addrStr, err)
	}

	return &proofSource{Address: addr, ChainID: chainID}, nil
}

// EVMSessionMethod implements protocol.SessionVerifier for EVM payment channels.
type EVMSessionMethod struct {
	chainID         uint64
	escrowContract  string
	recipient       string
	feePayer        bool
	channels        store.Store[store.ChannelState]
	saClient        saclient.SAClient
	perRequestCost  *big.Int
	minVoucherDelta *big.Int
	signer          Signer
	nonceProvider   NonceProvider
	deadline        *big.Int
	domainName      string
	domainVersion   string
	channelMu       sync.Map
}

// EVMSessionMethodConfig holds the configuration for creating an EVMSessionMethod.
type EVMSessionMethodConfig struct {
	// Required fields.
	Recipient string            // Payee wallet address.
	SAClient  saclient.SAClient // SA API client for on-chain operations.

	// Optional fields (zero value = use default).
	ChainID         uint64                          // Default: 196 (X Layer).
	EscrowContract  string                          // Default: DefaultEscrowContract.
	Signer          Signer                          // Payee signer for settle/close. Nil = settle/close disabled.
	Store           store.Store[store.ChannelState] // Default: in-memory store.
	PerRequestCost  *big.Int                        // Per-request deduction amount.
	MinVoucherDelta *big.Int                        // Minimum delta between consecutive vouchers.
	NonceProvider   NonceProvider                   // Default: UuidNonceProvider.
	Deadline        *big.Int                        // Default: U256 MAX (no expiry).
	DomainName      string                          // Default: "EVM Payment Channel".
	DomainVersion   string                          // Default: "1".
	FeePayer        bool                            // Whether payee sponsors gas fees.
}

// NewEVMSessionMethod creates a new EVMSessionMethod from the given config.
// Returns an error if required fields are missing.
func NewEVMSessionMethod(cfg EVMSessionMethodConfig) (*EVMSessionMethod, error) {
	if cfg.Recipient == "" {
		return nil, fmt.Errorf("recipient is required")
	}
	if cfg.SAClient == nil {
		return nil, fmt.Errorf("SA client is required")
	}
	if cfg.ChainID == 0 {
		cfg.ChainID = XLayerChainID
	}
	if cfg.EscrowContract == "" {
		cfg.EscrowContract = DefaultEscrowContract
	}
	if cfg.Store == nil {
		cfg.Store = store.NewMemoryStore[store.ChannelState]()
	}
	if cfg.NonceProvider == nil {
		cfg.NonceProvider = NewUuidNonceProvider()
	}
	if cfg.DomainName == "" {
		cfg.DomainName = DefaultDomainName
	}
	if cfg.DomainVersion == "" {
		cfg.DomainVersion = DefaultDomainVersion
	}
	return &EVMSessionMethod{
		chainID:         cfg.ChainID,
		escrowContract:  cfg.EscrowContract,
		recipient:       cfg.Recipient,
		feePayer:        cfg.FeePayer,
		channels:        cfg.Store,
		saClient:        cfg.SAClient,
		perRequestCost:  cfg.PerRequestCost,
		minVoucherDelta: cfg.MinVoucherDelta,
		signer:          cfg.Signer,
		nonceProvider:   cfg.NonceProvider,
		deadline:        cfg.Deadline,
		domainName:      cfg.DomainName,
		domainVersion:   cfg.DomainVersion,
	}, nil
}

// ChannelStore returns the underlying ChannelStore for use by middleware
// (e.g. for per-request balance deduction via store.DeductFromChannel).
func (m *EVMSessionMethod) ChannelStore() store.Store[store.ChannelState] {
	return m.channels
}

// DeductFromSession deducts the given amount from the channel's available
// balance. Returns the updated channel state or an error if insufficient
// balance, channel not found, or channel is closed/finalized.
func (m *EVMSessionMethod) DeductFromSession(ctx context.Context, channelID string, amount *big.Int) (*store.ChannelState, error) {
	mu := m.channelLock(channelID)
	mu.Lock()
	defer mu.Unlock()

	state, err := store.DeductFromChannel(ctx, m.channels, channelID, amount)
	if err != nil {
		return nil, err
	}
	return state, nil
}

// channelLock returns the mutex for a given channel, creating it if needed.
func (m *EVMSessionMethod) channelLock(channelID string) *sync.Mutex {
	v, _ := m.channelMu.LoadOrStore(channelID, &sync.Mutex{})
	return v.(*sync.Mutex)
}

// defaultDeadline returns the configured deadline or U256 MAX.
func (m *EVMSessionMethod) defaultDeadline() *big.Int {
	if m.deadline != nil {
		return new(big.Int).Set(m.deadline)
	}
	max := new(big.Int).Lsh(big.NewInt(1), 256)
	return max.Sub(max, big.NewInt(1))
}

// defaultNonceProvider returns the configured nonce provider.
func (m *EVMSessionMethod) defaultNonceProvider() NonceProvider {
	return m.nonceProvider
}

// SettleWithAuthorization performs an intermediate on-chain settlement.
// Reads the highest voucher from the store, signs a SettleAuthorization (payee),
// and submits to the SA API.
func (m *EVMSessionMethod) SettleWithAuthorization(ctx context.Context, channelID string) (*saclient.SessionReceipt, error) {
	if m.signer == nil {
		return nil, fmt.Errorf("payee signer not configured")
	}
	if m.saClient == nil {
		return nil, fmt.Errorf("SA client not configured")
	}

	mu := m.channelLock(channelID)
	mu.Lock()
	defer mu.Unlock()

	cs, err := m.channels.Get(ctx, channelID)
	if err != nil {
		return nil, fmt.Errorf("channel %q: %w", channelID, err)
	}
	if cs == nil {
		return nil, fmt.Errorf("channel %q not found", channelID)
	}
	if cs.HighestVoucherAmount == nil || cs.HighestVoucherAmount.Sign() == 0 {
		return nil, fmt.Errorf("channel %q has no voucher to settle", channelID)
	}
	if len(cs.HighestVoucherSignature) == 0 {
		return nil, fmt.Errorf("channel %q has no voucher signature", channelID)
	}

	channelIDBytes, err := hexDecode32(channelID)
	if err != nil {
		return nil, fmt.Errorf("invalid channelId: %w", err)
	}

	nonce, err := m.defaultNonceProvider().Allocate(m.signer.Address(), channelIDBytes)
	if err != nil {
		return nil, fmt.Errorf("allocate nonce: %w", err)
	}

	escrow := common.HexToAddress(cs.EscrowContract)
	deadline := m.defaultDeadline()

	signed, err := SignAuthorization(
		m.signer, "SettleAuthorization",
		channelIDBytes, cs.HighestVoucherAmount, nonce, deadline,
		escrow, cs.ChainID, m.domainName, m.domainVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("sign settle authorization: %w", err)
	}

	if err := ctx.Err(); err != nil {
		return nil, fmt.Errorf("context cancelled before SA settle: %w", err)
	}
	return m.saClient.SessionSettle(ctx, &saclient.SessionSettleRequest{
		Payload: saclient.SessionSettlePayload{
			Action:           ActionSettle,
			ChannelID:        channelID,
			CumulativeAmount: cs.HighestVoucherAmount.String(),
			VoucherSignature: "0x" + hex.EncodeToString(cs.HighestVoucherSignature),
			PayeeSignature:   "0x" + hex.EncodeToString(signed.Signature),
			Nonce:            nonce.String(),
			Deadline:         deadline.String(),
		},
	})
}

// CloseWithAuthorization performs a merchant-initiated channel close.
// Reads the highest voucher from the store, signs a CloseAuthorization (payee),
// and submits to the SA API. Removes the channel from the store on success.
func (m *EVMSessionMethod) CloseWithAuthorization(ctx context.Context, channelID string) (*saclient.SessionReceipt, error) {
	if m.signer == nil {
		return nil, fmt.Errorf("payee signer not configured")
	}

	mu := m.channelLock(channelID)
	mu.Lock()
	defer mu.Unlock()

	cs, err := m.channels.Get(ctx, channelID)
	if err != nil || cs == nil {
		return nil, protocol.ErrChannelNotFound(channelID)
	}

	channelIDBytes, err := hexDecode32(channelID)
	if err != nil {
		return nil, fmt.Errorf("invalid channelId: %w", err)
	}

	cumulativeAmount := cs.HighestVoucherAmount
	if cumulativeAmount == nil {
		cumulativeAmount = new(big.Int)
	}

	var voucherSig string
	if len(cs.HighestVoucherSignature) > 0 {
		voucherSig = "0x" + hex.EncodeToString(cs.HighestVoucherSignature)
	}

	return m.signAndSubmitClose(ctx, channelID, channelIDBytes, cumulativeAmount, voucherSig, common.HexToAddress(cs.EscrowContract), cs.ChainID)
}

// Method returns the payment method name.
func (m *EVMSessionMethod) Method() string {
	return MethodNameEVM
}

// Respond inspects the verified credential and returns a management response
// for open/topUp/close actions, or nil for voucher (resource-serving) actions.
// This mirrors mpp-rs's SessionMethod::respond() hook.
func (m *EVMSessionMethod) Respond(cred *protocol.PaymentCredential, receipt *protocol.Receipt) any {
	if cred == nil || cred.Payload == nil {
		return nil
	}
	var base Payload
	if err := json.Unmarshal([]byte(cred.Payload.Payload), &base); err != nil {
		return nil
	}
	switch base.Action {
	case ActionVoucher:
		return nil // proceed to handler, serve resource
	default:
		// open, topUp, close — management action, short-circuit
		return map[string]any{"status": "ok", "receipt": receipt}
	}
}

// ChallengeMethodDetails returns the EVM-specific method details embedded in the
// WWW-Authenticate challenge. Returns nil when no escrow contract is configured.
func (m *EVMSessionMethod) ChallengeMethodDetails() json.RawMessage {
	if m.escrowContract == "" {
		return nil
	}
	details := EVMSessionMethodDetails{
		EscrowContract: m.escrowContract,
	}
	if m.chainID != 0 {
		chainID := m.chainID
		details.ChainID = &chainID
	}
	{
		fp := m.feePayer
		details.FeePayer = &fp
	}
	if m.minVoucherDelta != nil {
		s := m.minVoucherDelta.String()
		details.MinVoucherDelta = &s
	}
	data, err := json.Marshal(details)
	if err != nil {
		return nil
	}
	return json.RawMessage(data)
}

// VerifySession validates a session payment credential against the session request.
// It dispatches to the appropriate handler based on the action field.
func (m *EVMSessionMethod) VerifySession(ctx context.Context, cred *protocol.PaymentCredential, request *protocol.SessionRequest) (*protocol.Receipt, error) {
	if cred == nil {
		return nil, protocol.ErrCredential("missing credential")
	}
	if cred.Payload == nil {
		return nil, protocol.ErrCredential("missing payload")
	}

	var challengeID string
	if cred.Echo != nil {
		challengeID = cred.Echo.ID
	}
	rawPayload := cred.Payload.Payload

	if rawPayload == "" {
		return nil, protocol.ErrPayload("empty payload")
	}
	var base Payload
	if err := json.Unmarshal([]byte(rawPayload), &base); err != nil {
		return nil, protocol.ErrPayload(fmt.Sprintf("invalid JSON: %v", err))
	}
	if base.Action == "" {
		return nil, protocol.ErrPayload("missing required field \"action\"")
	}

	switch base.Action {
	case ActionOpen:
		var p OpenPayload
		if err := json.Unmarshal([]byte(rawPayload), &p); err != nil {
			return nil, protocol.ErrPayload(fmt.Sprintf("open payload: %v", err))
		}
		return m.handleOpen(ctx, challengeID, cred, request, &p)
	case ActionTopUp:
		var p TopUpPayload
		if err := json.Unmarshal([]byte(rawPayload), &p); err != nil {
			return nil, protocol.ErrPayload(fmt.Sprintf("topup payload: %v", err))
		}
		return m.handleTopUp(ctx, challengeID, cred, request, &p)
	case ActionVoucher:
		var p VoucherPayload
		if err := json.Unmarshal([]byte(rawPayload), &p); err != nil {
			return nil, protocol.ErrPayload(fmt.Sprintf("voucher payload: %v", err))
		}
		return m.handleVoucher(ctx, challengeID, cred, request, &p)
	case ActionClose:
		var p ClosePayload
		if err := json.Unmarshal([]byte(rawPayload), &p); err != nil {
			return nil, protocol.ErrPayload(fmt.Sprintf("close payload: %v", err))
		}
		return m.handleClose(ctx, challengeID, cred, request, &p)
	default:
		return nil, protocol.ErrPayload(fmt.Sprintf("unknown action %q", base.Action))
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Payload handlers
// ─────────────────────────────────────────────────────────────────────────────

// handleOpen processes a channel open payload.
// It stores the initial channel state in the ChannelStore and returns a session receipt.
func (m *EVMSessionMethod) handleOpen(
	ctx context.Context,
	challengeID string,
	cred *protocol.PaymentCredential,
	request *protocol.SessionRequest,
	p *OpenPayload,
) (*protocol.Receipt, error) {
	if err := p.Validate(); err != nil {
		return nil, protocol.ErrPayload(err.Error())
	}
	if cred.Echo == nil {
		return nil, protocol.ErrCredential("open requires challenge echo")
	}
	deposit := p.EffectiveDeposit()

	chainID, escrowAddr, err := m.resolveChainAndEscrow(request)
	if err != nil {
		return nil, err
	}

	// Validate payee consistency: signer address must match configured recipient.
	if m.signer != nil && m.recipient != "" {
		signerAddr := strings.ToLower(m.signer.Address().Hex())
		recipientAddr := strings.ToLower(m.recipient)
		if signerAddr != recipientAddr {
			return nil, protocol.ErrCredential(
				fmt.Sprintf("payee signer address %s does not match recipient %s", signerAddr, recipientAddr),
			)
		}
	}

	var payer string
	if cred.Source != "" {
		parsed, parseErr := parseProofSource(cred.Source)
		if parseErr != nil {
			return nil, protocol.ErrCredential(
				fmt.Sprintf("invalid source for open payload: %v", parseErr),
			)
		}
		if parsed.ChainID != chainID {
			return nil, protocol.ErrCredential(
				fmt.Sprintf("source chain ID %d does not match expected %d", parsed.ChainID, chainID),
			)
		}
		payer = parsed.Address
	} else if p.Type == string(protocol.PayloadTypeTransaction) && p.Authorization != nil && p.Authorization.From != "" {
		addr, addrErr := ParseAddress(p.Authorization.From)
		if addrErr != nil {
			return nil, protocol.ErrCredential(
				fmt.Sprintf("invalid authorization.from: %v", addrErr),
			)
		}
		payer = addr
	}
	if payer == "" {
		return nil, protocol.ErrCredential("open payload: cannot determine payer (source or authorization.from required)")
	}

	var depositBig *big.Int
	if deposit != "" {
		var ok bool
		depositBig, ok = new(big.Int).SetString(deposit, 10)
		if !ok {
			return nil, protocol.ErrCredential("deposit is not a valid integer")
		}
	}

	if m.saClient == nil {
		return nil, protocol.ErrNetwork("SA client not configured")
	}

	channelKey := p.ChannelID

	// Pre-check: lock and verify no existing channel BEFORE calling SA,
	// to avoid irreversible remote side effects on duplicate/retry requests.
	mu := m.channelLock(channelKey)
	mu.Lock()
	defer mu.Unlock()

	existing, _ := m.channels.Get(ctx, channelKey)
	if existing != nil {
		return nil, protocol.ErrCredential(
			fmt.Sprintf("channel %q already exists", channelKey),
		)
	}

	var openPayload saclient.SessionOpenPayload
	if err := json.Unmarshal([]byte(cred.Payload.Payload), &openPayload); err != nil {
		return nil, protocol.ErrPayload(
			fmt.Sprintf("invalid open payload: %v", err),
		)
	}

	if err := ctx.Err(); err != nil {
		return nil, protocol.ErrNetwork(fmt.Sprintf("context cancelled before SA open: %v", err))
	}
	saReceipt, saErr := m.saClient.SessionOpen(ctx, &saclient.SessionOpenRequest{
		Challenge: cred.Echo,
		Payload:   openPayload,
		Source:    cred.Source,
	})
	if saErr != nil {
		return nil, protocol.ErrNetwork(
			fmt.Sprintf("SA session open failed: %v", saErr),
		)
	}
	if saReceipt.ChannelID != "" && saReceipt.ChannelID != channelKey {
		return nil, protocol.ErrCredential(
			fmt.Sprintf("SA returned channelId %q but expected %q", saReceipt.ChannelID, channelKey),
		)
	}

	// Resolve deposit: use payload value, or fall back to SA receipt (hash mode).
	if depositBig == nil && saReceipt.Deposit != "" {
		depositBig, _ = new(big.Int).SetString(saReceipt.Deposit, 10)
	}
	if depositBig == nil {
		return nil, protocol.ErrCredential("deposit not available from payload or SA receipt")
	}

	// Resolve authorizedSigner: use payload field if set, else default to payer.
	authorizedSigner := payer
	if p.AuthorizedSigner != "" && p.AuthorizedSigner != "0x0000000000000000000000000000000000000000" {
		authorizedSigner = p.AuthorizedSigner
	}

	// Extract initial voucher signature if present.
	// Transaction mode: voucherSignature field. Hash mode: signature field (overloaded per spec).
	// CLI may not send either yet — store nil in that case.
	var initialVoucherSig []byte
	var initialAmount *big.Int
	voucherSigHex := p.VoucherSignature // transaction mode
	if voucherSigHex == "" && p.Type == string(protocol.PayloadTypeHash) {
		// hash mode: signature field is the voucher sig (per spec)
		voucherSigHex = p.Signature
	}
	if voucherSigHex != "" {
		sigBytes, sigErr := hex.DecodeString(strings.TrimPrefix(voucherSigHex, "0x"))
		if sigErr != nil {
			return nil, protocol.ErrSig(fmt.Sprintf("invalid voucher signature hex: %v", sigErr))
		}
		if err := ValidateVoucherSignature(sigBytes); err != nil {
			return nil, protocol.ErrSig(err.Error())
		}
		initialVoucherSig = sigBytes
	}
	if p.CumulativeAmount != "" {
		if amt, ok := new(big.Int).SetString(p.CumulativeAmount, 10); ok {
			initialAmount = amt
		}
	}

	// Verify initial voucher signature via EIP-712 ecrecover.
	if initialVoucherSig != nil && initialAmount != nil {
		channelIDBytes, decErr := hexDecode32(channelKey)
		if decErr != nil {
			return nil, protocol.ErrCredential(fmt.Sprintf("invalid channelId for voucher verify: %v", decErr))
		}
		escrow := common.HexToAddress(escrowAddr)
		signerAddr := common.HexToAddress(authorizedSigner)
		if !VerifyVoucher(escrow, chainID, channelIDBytes, initialAmount, initialVoucherSig, signerAddr, m.domainName, m.domainVersion) {
			return nil, protocol.ErrSig("initial voucher signature verification failed")
		}
	}

	cs := &store.ChannelState{
		ChannelID:               channelKey,
		ChainID:                 chainID,
		EscrowContract:          escrowAddr,
		Payer:                   payer,
		Payee:                   m.recipient,
		AuthorizedSigner:        authorizedSigner,
		Deposit:                 depositBig,
		HighestVoucherAmount:    initialAmount,
		HighestVoucherSignature: initialVoucherSig,
		Spent:                   new(big.Int),
		CreatedAt:               time.Now().UTC().Format(time.RFC3339),
	}
	err = m.channels.Put(ctx, channelKey, cs)
	if err != nil {
		return nil, fmt.Errorf("channel opened on-chain (ref=%s) but store write failed: %w",
			saReceipt.Reference, err)
	}

	receipt := NewSessionReceipt(channelKey, "0", escrowAddr, StatusOpen, saReceipt.Reference)
	return receipt.ToBaseReceipt(challengeID)
}

// handleTopUp processes a channel top-up payload.
// It increases the deposit in the ChannelStore.
func (m *EVMSessionMethod) handleTopUp(
	ctx context.Context,
	challengeID string,
	cred *protocol.PaymentCredential,
	_ *protocol.SessionRequest,
	p *TopUpPayload,
) (*protocol.Receipt, error) {
	if err := p.Validate(); err != nil {
		return nil, protocol.ErrPayload(err.Error())
	}
	addAmount, ok := new(big.Int).SetString(p.AdditionalDeposit, 10)
	if !ok {
		return nil, protocol.ErrCredential("topup amount is not a valid integer")
	}

	if m.saClient == nil {
		return nil, protocol.ErrNetwork("SA client not configured")
	}

	// Validate local channel state BEFORE calling SA to avoid irreversible
	// remote side effects when local state is invalid.
	mu := m.channelLock(p.ChannelID)
	mu.Lock()
	defer mu.Unlock()

	cs, err := m.channels.Get(ctx, p.ChannelID)
	if err != nil || cs == nil {
		return nil, protocol.ErrChannelNotFound(p.ChannelID)
	}
	if cs.Finalized {
		return nil, protocol.ErrChannelClosed()
	}

	var topUpPayload saclient.SessionTopUpPayload
	if err := json.Unmarshal([]byte(cred.Payload.Payload), &topUpPayload); err != nil {
		return nil, protocol.ErrPayload(
			fmt.Sprintf("invalid topup payload: %v", err),
		)
	}

	if err := ctx.Err(); err != nil {
		return nil, protocol.ErrNetwork(fmt.Sprintf("context cancelled before SA topup: %v", err))
	}
	saReceipt, saErr := m.saClient.SessionTopUp(ctx, &saclient.SessionTopUpRequest{
		Challenge: cred.Echo,
		Payload:   topUpPayload,
		Source:    cred.Source,
	})
	if saErr != nil {
		return nil, protocol.ErrNetwork(
			fmt.Sprintf("SA session topup failed: %v", saErr),
		)
	}

	if cs.Deposit == nil {
		cs.Deposit = new(big.Int)
	}
	cs.Deposit = new(big.Int).Add(cs.Deposit, addAmount)
	if err := m.channels.Put(ctx, p.ChannelID, cs); err != nil {
		var ref string
		if saReceipt != nil {
			ref = saReceipt.Reference
		}
		return nil, fmt.Errorf("topup confirmed on-chain (ref=%s) but store write failed: %w", ref, err)
	}

	cumulative := "0"
	if cs.HighestVoucherAmount != nil {
		cumulative = cs.HighestVoucherAmount.String()
	}
	escrow := cs.EscrowContract
	var ref string
	if saReceipt != nil {
		ref = saReceipt.Reference
	}
	receipt := NewSessionReceipt(p.ChannelID, cumulative, escrow, StatusOpen, ref)
	return receipt.ToBaseReceipt(challengeID)
}

// handleVoucher processes a voucher payload (incremental payment).
//
// Two paths:
//   - New voucher (amount > HighestVoucherAmount): validate fields, call SA API,
//     update HighestVoucherAmount, then deduct perRequestCost.
//   - Replay voucher (amount == HighestVoucherAmount): skip sig check + SA API,
//     just deduct perRequestCost from available balance.
//
// available = HighestVoucherAmount - Spent. If available < perRequestCost -> error (402).
func (m *EVMSessionMethod) handleVoucher(
	ctx context.Context,
	challengeID string,
	_ *protocol.PaymentCredential,
	_ *protocol.SessionRequest,
	p *VoucherPayload,
) (*protocol.Receipt, error) {
	if err := p.Validate(); err != nil {
		return nil, protocol.ErrPayload(err.Error())
	}
	voucherAmount := p.CumulativeAmount

	newAmount, ok := new(big.Int).SetString(voucherAmount, 10)
	if !ok {
		return nil, protocol.ErrAmount("voucher amount is not a valid integer")
	}

	sigBytes, sigErr := hex.DecodeString(strings.TrimPrefix(p.Signature, "0x"))
	if sigErr != nil {
		return nil, protocol.ErrSig(
			fmt.Sprintf("invalid signature hex: %v", sigErr),
		)
	}

	// Step 1: Per-channel lock.
	mu := m.channelLock(p.ChannelID)
	mu.Lock()
	defer mu.Unlock()

	// Step 2: Load channel state.
	cs, err := m.channels.Get(ctx, p.ChannelID)
	if err != nil || cs == nil {
		return nil, protocol.ErrChannelNotFound(p.ChannelID)
	}
	if cs.Finalized {
		return nil, protocol.ErrChannelClosed()
	}

	// Step 3: Amount guard — must not exceed deposit.
	deposit := cs.Deposit
	if deposit == nil {
		deposit = new(big.Int)
	}
	if newAmount.Cmp(deposit) > 0 {
		return nil, protocol.ErrExceedsDeposit(voucherAmount, deposit.String())
	}

	prev := cs.HighestVoucherAmount
	if prev == nil {
		prev = new(big.Int)
	}

	// Step 4: Idempotent / ordering check.
	isReplay := newAmount.Cmp(prev) == 0 && bytes.Equal(sigBytes, cs.HighestVoucherSignature)
	if !isReplay && newAmount.Cmp(prev) <= 0 {
		return nil, protocol.ErrAmount(
			fmt.Sprintf("voucher amount %s is not strictly greater than current %s", voucherAmount, prev.String()),
		)
	}

	if !isReplay {
		// Step 5: Min delta throttle (check both instance and channel-level config).
		minDelta := m.minVoucherDelta
		if minDelta == nil && cs.MinVoucherDelta != nil {
			minDelta = cs.MinVoucherDelta
		}
		if minDelta != nil && minDelta.Sign() > 0 {
			delta := new(big.Int).Sub(newAmount, prev)
			if delta.Cmp(minDelta) < 0 {
				return nil, protocol.ErrDeltaTooSmall(delta.String(), minDelta.String())
			}
		}

		// Step 6: Signature validation + EIP-712 verify.
		if err := ValidateVoucherSignature(sigBytes); err != nil {
			return nil, protocol.ErrSig(err.Error())
		}

		channelIDBytes, decErr := hexDecode32(p.ChannelID)
		if decErr != nil {
			return nil, protocol.ErrCredential(
				fmt.Sprintf("invalid channelId: %v", decErr),
			)
		}
		escrowAddr := common.HexToAddress(cs.EscrowContract)

		// Determine expected signer from channel state (no source needed).
		expectedSigner := cs.AuthorizedSigner
		if expectedSigner == "" || expectedSigner == "0x0000000000000000000000000000000000000000" {
			expectedSigner = cs.Payer
		}
		signerAddr := common.HexToAddress(expectedSigner)

		if !VerifyVoucher(escrowAddr, cs.ChainID, channelIDBytes, newAmount, sigBytes, signerAddr, m.domainName, m.domainVersion) {
			return nil, protocol.ErrSig("voucher signature verification failed")
		}

		// Step 7: Atomic store update (already under per-channel lock).
		cs.HighestVoucherAmount = new(big.Int).Set(newAmount)
		cs.HighestVoucherSignature = make([]byte, len(sigBytes))
		copy(cs.HighestVoucherSignature, sigBytes)
		if err := m.channels.Put(ctx, p.ChannelID, cs); err != nil {
			return nil, protocol.ErrNetwork(fmt.Sprintf("store write: %v", err))
		}
	}

	// Deduct per-request cost (both new and replay).
	if m.perRequestCost != nil && m.perRequestCost.Sign() > 0 {
		_, deductErr := store.DeductFromChannel(ctx, m.channels, p.ChannelID, m.perRequestCost)
		if deductErr != nil {
			return nil, protocol.ErrInsufficientBalance(deductErr.Error(), m.perRequestCost.String())
		}
	}

	// Re-read for receipt.
	updated, _ := m.channels.Get(ctx, p.ChannelID)
	cumulative := "0"
	escrow := ""
	if updated != nil {
		if updated.HighestVoucherAmount != nil {
			cumulative = updated.HighestVoucherAmount.String()
		}
		escrow = updated.EscrowContract
	}
	receipt := NewSessionReceipt(p.ChannelID, cumulative, escrow, StatusOpen, "")
	return receipt.ToBaseReceipt(challengeID)
}

// handleClose processes a channel close payload.
//
// Close validation rules:
//   - client amount == stored amount → verify EIP-712 sig
//   - client amount > stored amount  → enforce minVoucherDelta, verify EIP-712 sig, update store
//   - client amount < stored amount  → reject
//
// The server always settles with the final (possibly updated) stored voucher.
func (m *EVMSessionMethod) handleClose(
	ctx context.Context,
	challengeID string,
	_ *protocol.PaymentCredential,
	_ *protocol.SessionRequest,
	p *ClosePayload,
) (*protocol.Receipt, error) {
	if err := p.Validate(); err != nil {
		return nil, protocol.ErrPayload(err.Error())
	}
	channelIDBytes, decErr := hexDecode32(p.ChannelID)
	if decErr != nil {
		return nil, protocol.ErrCredential(
			fmt.Sprintf("invalid channelId: %v", decErr),
		)
	}

	clientAmount, ok := new(big.Int).SetString(p.CumulativeAmount, 10)
	if !ok {
		return nil, protocol.ErrAmount("close cumulativeAmount is not a valid integer")
	}

	clientSigBytes, sigErr := hex.DecodeString(strings.TrimPrefix(p.Signature, "0x"))
	if sigErr != nil {
		return nil, protocol.ErrSig(
			fmt.Sprintf("invalid signature hex: %v", sigErr),
		)
	}

	mu := m.channelLock(p.ChannelID)
	mu.Lock()
	defer mu.Unlock()

	cs, err := m.channels.Get(ctx, p.ChannelID)
	if err != nil || cs == nil {
		return nil, protocol.ErrChannelNotFound(p.ChannelID)
	}
	if cs.Finalized {
		return nil, protocol.ErrChannelClosed()
	}

	// Guard: cumulativeAmount must not exceed deposit.
	deposit := cs.Deposit
	if deposit == nil {
		deposit = new(big.Int)
	}
	if clientAmount.Cmp(deposit) > 0 {
		return nil, protocol.ErrExceedsDeposit(clientAmount.String(), deposit.String())
	}

	storedAmount := cs.HighestVoucherAmount
	if storedAmount == nil {
		storedAmount = new(big.Int)
	}

	cmp := clientAmount.Cmp(storedAmount)
	if cmp < 0 {
		// Non-increasing close amount → delta-too-small.
		return nil, protocol.VerificationErrorDeltaTooSmall(
			fmt.Sprintf("close cumulativeAmount %s is less than stored %s", clientAmount.String(), storedAmount.String()),
		)
	}

	// Enforce minVoucherDelta when client sends higher amount than stored.
	if cmp > 0 {
		minDelta := m.minVoucherDelta
		if minDelta == nil && cs.MinVoucherDelta != nil {
			minDelta = cs.MinVoucherDelta
		}
		if minDelta != nil && minDelta.Sign() > 0 {
			delta := new(big.Int).Sub(clientAmount, storedAmount)
			if delta.Cmp(minDelta) < 0 {
				return nil, protocol.ErrDeltaTooSmall(delta.String(), minDelta.String())
			}
		}
	}

	// Always verify the client's voucher signature via ecrecover.
	if err := ValidateVoucherSignature(clientSigBytes); err != nil {
		return nil, protocol.ErrSig(err.Error())
	}
	escrowAddr := common.HexToAddress(cs.EscrowContract)
	expectedSigner := cs.AuthorizedSigner
	if expectedSigner == "" || expectedSigner == "0x0000000000000000000000000000000000000000" {
		expectedSigner = cs.Payer
	}
	if !VerifyVoucher(escrowAddr, cs.ChainID, channelIDBytes, clientAmount, clientSigBytes, common.HexToAddress(expectedSigner), m.domainName, m.domainVersion) {
		return nil, protocol.ErrSig("close voucher signature verification failed")
	}

	// Update store if client sent a higher voucher.
	if cmp > 0 {
		cs.HighestVoucherAmount = new(big.Int).Set(clientAmount)
		cs.HighestVoucherSignature = make([]byte, len(clientSigBytes))
		copy(cs.HighestVoucherSignature, clientSigBytes)
		if err := m.channels.Put(ctx, p.ChannelID, cs); err != nil {
			return nil, protocol.ErrNetwork(fmt.Sprintf("store write: %v", err))
		}
	}

	// Settle with stored voucher (now the final highest).
	escrow := cs.EscrowContract
	settleAmount := cs.HighestVoucherAmount
	if settleAmount == nil {
		settleAmount = new(big.Int)
	}
	var settleVoucherSig string
	if len(cs.HighestVoucherSignature) > 0 {
		settleVoucherSig = "0x" + hex.EncodeToString(cs.HighestVoucherSignature)
	}

	saReceipt, closeErr := m.signAndSubmitClose(ctx, p.ChannelID, channelIDBytes, settleAmount, settleVoucherSig, common.HexToAddress(escrow), cs.ChainID)
	if closeErr != nil {
		return nil, protocol.ErrNetwork(
			fmt.Sprintf("close failed: %v", closeErr),
		)
	}

	var ref string
	if saReceipt != nil {
		ref = saReceipt.Reference
	}
	receipt := NewSessionReceipt(p.ChannelID, settleAmount.String(), escrow, StatusClosed, ref)
	return receipt.ToBaseReceipt(challengeID)
}

// signAndSubmitClose signs a CloseAuthorization, submits to SA API, and deletes the channel.
// Caller must hold the channel lock. channelIDBytes must match channelID.
func (m *EVMSessionMethod) signAndSubmitClose(
	ctx context.Context,
	channelID string,
	channelIDBytes [32]byte,
	cumulativeAmount *big.Int,
	voucherSignature string,
	escrowContract common.Address,
	chainID uint64,
) (*saclient.SessionReceipt, error) {
	if m.signer == nil {
		return nil, fmt.Errorf("payee signer not configured")
	}
	if m.saClient == nil {
		return nil, fmt.Errorf("SA client not configured")
	}

	var payeeSig string
	var nonceStr string
	var deadlineStr string

	nonce, err := m.defaultNonceProvider().Allocate(m.signer.Address(), channelIDBytes)
	if err != nil {
		return nil, fmt.Errorf("allocate nonce: %w", err)
	}
	deadline := m.defaultDeadline()

	signed, err := SignAuthorization(
		m.signer, "CloseAuthorization",
		channelIDBytes, cumulativeAmount, nonce, deadline,
		escrowContract, chainID, m.domainName, m.domainVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("sign close authorization: %w", err)
	}
	payeeSig = "0x" + hex.EncodeToString(signed.Signature)
	nonceStr = nonce.String()
	deadlineStr = deadline.String()

	if err := ctx.Err(); err != nil {
		return nil, fmt.Errorf("context cancelled before SA close: %w", err)
	}
	receipt, saErr := m.saClient.SessionClose(ctx, &saclient.SessionCloseRequest{
		Payload: saclient.SessionClosePayload{
			Action:           ActionClose,
			ChannelID:        channelID,
			CumulativeAmount: cumulativeAmount.String(),
			VoucherSignature: voucherSignature,
			PayeeSignature:   payeeSig,
			Nonce:            nonceStr,
			Deadline:         deadlineStr,
		},
	})
	if saErr != nil {
		return nil, saErr
	}

	if err := m.channels.Delete(ctx, channelID); err != nil {
		// SA close succeeded but local delete failed.
		// Mark channel finalized so it cannot be reused.
		if cs, getErr := m.channels.Get(ctx, channelID); getErr == nil && cs != nil {
			cs.Finalized = true
			m.channels.Put(ctx, channelID, cs) //nolint:errcheck // best-effort fallback
		}
	} else {
		m.channelMu.Delete(channelID)
	}
	return receipt, nil
}

// ─────────────────────────────────────────────────────────────────────────────
// Misc helpers
// ─────────────────────────────────────────────────────────────────────────────

// resolveChainAndEscrow returns the effective chain ID and escrow contract address
// by merging request method_details with instance-level configuration.
// Request method_details take precedence; instance values are used as fallback.
func (m *EVMSessionMethod) resolveChainAndEscrow(request *protocol.SessionRequest) (uint64, string, error) {
	chainID := m.chainID
	escrowAddr := m.escrowContract

	if request != nil && len(request.MethodDetails) > 0 {
		details := parseSessionDetails(request.MethodDetails)
		if details != nil {
			if details.ChainID != nil {
				chainID = *details.ChainID
			}
			if details.EscrowContract != "" {
				escrowAddr = details.EscrowContract
			}
		}
	}

	if escrowAddr == "" {
		return 0, "", protocol.ErrCredential(
			"no escrow contract configured",
		)
	}
	return chainID, escrowAddr, nil
}

// hexDecode32 hex-decodes (with optional "0x" prefix) and returns a [32]byte.
func hexDecode32(s string) ([32]byte, error) {
	b, err := hex.DecodeString(strings.TrimPrefix(s, "0x"))
	if err != nil {
		return [32]byte{}, fmt.Errorf("hex decode: %w", err)
	}
	if len(b) > 32 {
		return [32]byte{}, fmt.Errorf("value too long: got %d bytes, want ≤32", len(b))
	}
	var out [32]byte
	copy(out[32-len(b):], b)
	return out, nil
}
