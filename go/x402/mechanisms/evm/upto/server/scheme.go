// Package server provides the server-side scheme implementation for the upto
// EVM payment scheme.
//
// The server is responsible for two concerns:
//
//  1. Building PaymentRequirements (via ParsePrice + EnhancePaymentRequirements,
//     wired into x402ResourceServer.BuildPaymentRequirements). The upto server
//     stamps `assetTransferMethod: "permit2"` on every requirements.Extra and
//     propagates `facilitatorAddress` from supportedKind.Extra so the client
//     can sign the witness with the authorized facilitator.
//
//  2. Validating incoming X-PAYMENT payloads (via ValidateUptoPayload — see
//     validate.go). This is the structural / cross-field validation that runs
//     before the payload is forwarded to the facilitator. The on-chain
//     simulation + signature verification stays on the facilitator side.
//
// Construction pattern (mirrors exact/server):
//
//	server := x402.Newx402ResourceServer(...)
//	server.Register("eip155:*", uptoserver.NewUptoEvmScheme())
package server

import (
	"context"
	"errors"
	"fmt"
	"math/big"
	"strconv"
	"strings"

	"github.com/okx/payments/go/x402"
	"github.com/okx/payments/go/x402/mechanisms/evm"
	uptoclient "github.com/okx/payments/go/x402/mechanisms/evm/upto/client"
	"github.com/okx/payments/go/x402/types"
)

// ExtraFacilitatorAddressKey is the requirements.Extra key the upto server
// writes (via EnhancePaymentRequirements) so the upto client can sign the
// witness with the authorized facilitator address. Re-exported from the
// upto/client package — keeping these in lock-step is a safety invariant:
// the client reads exactly this key, the server writes exactly this key.
// (Importing client into server is cycle-free; client does not import server.)
const ExtraFacilitatorAddressKey = uptoclient.ExtraFacilitatorAddressKey

// AssetTransferMethodKey is the requirements.Extra key the upto server stamps
// to indicate the on-chain settlement method (always set to "permit2").
const AssetTransferMethodKey = "assetTransferMethod"

// UptoEvmScheme implements the x402.SchemeNetworkServer interface for the
// upto EVM payment scheme. Server-side, no signer is required — the scheme
// only builds requirements + validates incoming payloads structurally.
type UptoEvmScheme struct {
	moneyParsers []x402.MoneyParser
}

// NewUptoEvmScheme creates a new UptoEvmScheme. No configuration is required:
// the scheme always emits `assetTransferMethod: "permit2"` and propagates
// `facilitatorAddress` from the supportedKind.
func NewUptoEvmScheme() *UptoEvmScheme {
	return &UptoEvmScheme{
		moneyParsers: []x402.MoneyParser{},
	}
}

// Scheme returns the scheme identifier. Always returns evm.SchemeUpto ("upto").
func (s *UptoEvmScheme) Scheme() string {
	return evm.SchemeUpto
}

// RegisterMoneyParser registers a custom money parser in the parser chain.
// Multiple parsers can be registered — they will be tried in registration order.
// Each parser receives a decimal amount (e.g., 1.50 for $1.50).
// If a parser returns nil, the next parser in the chain will be tried.
// The default parser is always the final fallback.
//
// Mirrors ExactEvmScheme.RegisterMoneyParser.
func (s *UptoEvmScheme) RegisterMoneyParser(parser x402.MoneyParser) *UptoEvmScheme {
	s.moneyParsers = append(s.moneyParsers, parser)
	return s
}

// ParsePrice parses a price string and converts it to an asset amount (V2).
// If price is already an AssetAmount, returns it directly.
// If price is Money (string | number), parses to decimal and tries custom parsers.
// Falls back to default conversion if all custom parsers return nil.
//
// Mirrors ExactEvmScheme.ParsePrice — same chain semantics, but the default
// conversion stamps `assetTransferMethod: "permit2"` so all upto requirements
// declare the permit2 transfer method.
func (s *UptoEvmScheme) ParsePrice(price x402.Price, network x402.Network) (x402.AssetAmount, error) {
	// If already an AssetAmount (map with "amount" and "asset"), return it directly.
	if priceMap, ok := price.(map[string]interface{}); ok {
		if amountVal, hasAmount := priceMap["amount"]; hasAmount {
			amountStr, ok := amountVal.(string)
			if !ok {
				return x402.AssetAmount{}, errors.New(ErrAmountMustBeString)
			}

			asset := ""
			if assetVal, hasAsset := priceMap["asset"]; hasAsset {
				if assetStr, ok := assetVal.(string); ok {
					asset = assetStr
				}
			}

			if asset == "" {
				return x402.AssetAmount{}, errors.New(ErrAssetAddressRequired)
			}

			extra := make(map[string]interface{})
			if extraVal, hasExtra := priceMap["extra"]; hasExtra {
				if extraMap, ok := extraVal.(map[string]interface{}); ok {
					extra = extraMap
				}
			}

			return x402.AssetAmount{
				Amount: amountStr,
				Asset:  asset,
				Extra:  extra,
			}, nil
		}
	}

	// Parse Money — preserve the original decimal string for atomic conversion
	// (avoids float64 precision loss for >6-decimal tokens or large values) and
	// also derive a float64 for the custom-parser API + integer-base-unit hint.
	amountStr, decimalAmount, err := s.parseMoneyToDecimal(price)
	if err != nil {
		return x402.AssetAmount{}, err
	}

	// Try each custom money parser in order.
	for _, parser := range s.moneyParsers {
		result, err := parser(decimalAmount, network)
		if err != nil {
			// Parser returned an error, skip it.
			continue
		}
		if result != nil {
			// Parser handled the conversion.
			return *result, nil
		}
		// Parser returned nil, try next one.
	}

	// All custom parsers returned nil, use default conversion.
	return s.defaultMoneyConversion(amountStr, decimalAmount, network)
}

// parseMoneyToDecimal converts Money (string | number) to a (string, float64)
// pair. The string is the cleaned decimal representation used for exact atomic
// conversion via big.Int; the float64 is exposed to custom money parsers and
// used only for the "already in smallest unit" heuristic.
func (s *UptoEvmScheme) parseMoneyToDecimal(price x402.Price) (string, float64, error) {
	switch v := price.(type) {
	case string:
		// Remove currency symbols.
		cleanPrice := strings.TrimSpace(v)
		cleanPrice = strings.TrimPrefix(cleanPrice, "$")
		cleanPrice = strings.TrimSuffix(cleanPrice, " USD")
		cleanPrice = strings.TrimSuffix(cleanPrice, " USDC")
		cleanPrice = strings.TrimSpace(cleanPrice)

		amount, err := strconv.ParseFloat(cleanPrice, 64)
		if err != nil {
			return "", 0, fmt.Errorf(ErrFailedToParsePrice+": '%s': %w", v, err)
		}
		return cleanPrice, amount, nil

	case float64:
		// FormatFloat(-1) emits the shortest representation that round-trips.
		return strconv.FormatFloat(v, 'f', -1, 64), v, nil

	case int:
		return strconv.Itoa(v), float64(v), nil

	case int64:
		return strconv.FormatInt(v, 10), float64(v), nil

	default:
		return "", 0, fmt.Errorf(ErrUnsupportedPriceType+": %T", price)
	}
}

// defaultMoneyConversion converts decimal amount to the network's default
// asset AssetAmount. Always stamps `assetTransferMethod: "permit2"` on the
// result. `amountStr` is the cleaned decimal string used for exact atomic
// conversion; `amountF64` is only the "already-in-smallest-unit" detection hint.
func (s *UptoEvmScheme) defaultMoneyConversion(amountStr string, amountF64 float64, network x402.Network) (x402.AssetAmount, error) {
	networkStr := string(network)

	config, err := evm.GetNetworkConfig(networkStr)
	if err != nil {
		return x402.AssetAmount{}, err
	}

	if config.DefaultAsset.Address == "" {
		return x402.AssetAmount{}, fmt.Errorf("no default stablecoin configured for network %s; use RegisterMoneyParser or specify an explicit AssetAmount", networkStr)
	}

	// Upto always uses Permit2. Include name/version only when the asset
	// supports EIP-2612 (so the client can sign a gas-sponsored permit).
	extra := map[string]interface{}{
		AssetTransferMethodKey: "permit2",
	}
	if config.DefaultAsset.SupportsEip2612 {
		extra["name"] = config.DefaultAsset.Name
		extra["version"] = config.DefaultAsset.Version
	}

	// Heuristic: if the input is a non-fractional integer at-or-above one
	// whole token in base units, treat as already-in-smallest-unit. Float
	// comparison is fine here — only used as a routing hint, the actual amount
	// is sourced from amountStr below.
	oneUnit := float64(1)
	for i := 0; i < config.DefaultAsset.Decimals; i++ {
		oneUnit *= 10
	}

	if amountF64 >= oneUnit && amountF64 == float64(int64(amountF64)) {
		return x402.AssetAmount{
			Asset:  config.DefaultAsset.Address,
			Amount: fmt.Sprintf("%.0f", amountF64),
			Extra:  extra,
		}, nil
	}

	// Convert the cleaned decimal STRING directly to atomic units via big.Int.
	// Avoids float64 precision loss + the previous hardcoded %.6f rounding,
	// which silently truncated > 6-decimal tokens (e.g. USDM at 18 decimals).
	parsedAmount, err := evm.ParseAmount(amountStr, config.DefaultAsset.Decimals)
	if err != nil {
		return x402.AssetAmount{}, fmt.Errorf(ErrFailedToConvertAmount+": %w", err)
	}

	return x402.AssetAmount{
		Asset:  config.DefaultAsset.Address,
		Amount: parsedAmount.String(),
		Extra:  extra,
	}, nil
}

// EnhancePaymentRequirements adds scheme-specific enhancements to V2 payment
// requirements:
//
//   - Always stamps `extra.assetTransferMethod = "permit2"`.
//   - If supportedKind.Extra carries `facilitatorAddress`, propagates it into
//     requirements.Extra (checksummed via NormalizeAddress).
//
// Asset / amount normalization follows the same pattern as exact/server.
func (s *UptoEvmScheme) EnhancePaymentRequirements(
	ctx context.Context,
	requirements types.PaymentRequirements,
	supportedKind types.SupportedKind,
	extensionKeys []string,
) (types.PaymentRequirements, error) {
	networkStr := string(requirements.Network)

	// Resolve asset (default if unspecified).
	var assetInfo *evm.AssetInfo
	var err error
	if requirements.Asset != "" {
		assetInfo, err = evm.GetAssetInfo(networkStr, requirements.Asset)
		if err != nil {
			return requirements, err
		}
	} else {
		assetInfo, err = evm.GetAssetInfo(networkStr, "")
		if err != nil {
			return requirements, fmt.Errorf(ErrNoAssetSpecified+": %w", err)
		}
		requirements.Asset = assetInfo.Address
	}

	// Convert decimal amount to smallest unit if needed.
	if requirements.Amount != "" && strings.Contains(requirements.Amount, ".") {
		amount, perr := evm.ParseAmount(requirements.Amount, assetInfo.Decimals)
		if perr != nil {
			return requirements, fmt.Errorf(ErrFailedToParseAmount+": %w", perr)
		}
		requirements.Amount = amount.String()
	}

	if requirements.Extra == nil {
		requirements.Extra = make(map[string]interface{})
	}

	// Upto: always permit2.
	requirements.Extra[AssetTransferMethodKey] = "permit2"

	// Include EIP-2612 name/version only for tokens that support it.
	if assetInfo.SupportsEip2612 {
		if _, ok := requirements.Extra["name"]; !ok {
			requirements.Extra["name"] = assetInfo.Name
		}
		if _, ok := requirements.Extra["version"]; !ok {
			requirements.Extra["version"] = assetInfo.Version
		}
	}

	// Propagate facilitatorAddress from supportedKind.Extra. Only set when
	// non-empty (so clients see nothing when the facilitator hasn't published
	// its address).
	if supportedKind.Extra != nil {
		if facAddr, ok := supportedKind.Extra[ExtraFacilitatorAddressKey].(string); ok && facAddr != "" {
			requirements.Extra[ExtraFacilitatorAddressKey] = evm.NormalizeAddress(facAddr)
		}
	}

	// Copy extra extension keys from supportedKind if provided (parity with
	// exact/server.EnhancePaymentRequirements).
	if supportedKind.Extra != nil {
		for _, key := range extensionKeys {
			if val, ok := supportedKind.Extra[key]; ok {
				requirements.Extra[key] = val
			}
		}
	}

	return requirements, nil
}

// GetDisplayAmount formats an amount for display. Mirrors exact/server.
func (s *UptoEvmScheme) GetDisplayAmount(amount string, network string, asset string) (string, error) {
	assetInfo, err := evm.GetAssetInfo(network, asset)
	if err != nil {
		return "", err
	}

	amountBig, ok := new(big.Int).SetString(amount, 10)
	if !ok {
		return "", fmt.Errorf("invalid amount: %s", amount)
	}

	formatted := evm.FormatAmount(amountBig, assetInfo.Decimals)
	return "$" + formatted + " USDC", nil
}

// ValidatePaymentRequirements validates that requirements are valid for this
// scheme. All EVM networks are supported — this validates required fields only.
func (s *UptoEvmScheme) ValidatePaymentRequirements(requirements x402.PaymentRequirements) error {
	networkStr := string(requirements.Network)

	if !evm.IsValidAddress(requirements.PayTo) {
		return fmt.Errorf(ErrInvalidPayToAddress+": %s", requirements.PayTo)
	}

	if requirements.Amount == "" {
		return errors.New(ErrAmountRequired)
	}

	amount, ok := new(big.Int).SetString(requirements.Amount, 10)
	if !ok || amount.Sign() <= 0 {
		return fmt.Errorf(ErrInvalidAmount+": %s", requirements.Amount)
	}

	if requirements.Asset != "" && !evm.IsValidAddress(requirements.Asset) {
		// Try to look it up (only works for networks with default assets).
		_, err := evm.GetAssetInfo(networkStr, requirements.Asset)
		if err != nil {
			return fmt.Errorf(ErrInvalidAsset+": %s", requirements.Asset)
		}
	}

	return nil
}

// ConvertToTokenAmount converts a decimal amount to token smallest unit.
func (s *UptoEvmScheme) ConvertToTokenAmount(decimalAmount string, network string) (string, error) {
	config, err := evm.GetNetworkConfig(network)
	if err != nil {
		return "", err
	}

	amount, err := evm.ParseAmount(decimalAmount, config.DefaultAsset.Decimals)
	if err != nil {
		return "", err
	}

	return amount.String(), nil
}

// ConvertFromTokenAmount converts from token smallest unit to decimal.
func (s *UptoEvmScheme) ConvertFromTokenAmount(tokenAmount string, network string) (string, error) {
	config, err := evm.GetNetworkConfig(network)
	if err != nil {
		return "", err
	}

	amount, ok := new(big.Int).SetString(tokenAmount, 10)
	if !ok {
		return "", fmt.Errorf(ErrInvalidTokenAmount+": %s", tokenAmount)
	}

	return evm.FormatAmount(amount, config.DefaultAsset.Decimals), nil
}

// GetSupportedNetworks returns the list of supported networks.
func (s *UptoEvmScheme) GetSupportedNetworks() []string {
	networks := make([]string, 0, len(evm.NetworkConfigs))
	for network := range evm.NetworkConfigs {
		networks = append(networks, network)
	}
	return networks
}
