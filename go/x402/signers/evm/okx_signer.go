package evm

import (
	"errors"

	x402evm "github.com/okx/payments/go/x402/mechanisms/evm"
)

// TEEConfig holds configuration for TEE-based signing (Phase 2).
type TEEConfig struct {
	Endpoint  string
	AccessKey string
}

// OKXSignerConfig configures the OKX signer.
// Provide either PrivateKey (Phase 1) or TEE (Phase 2).
type OKXSignerConfig struct {
	// PrivateKey is a hex-encoded ECDSA private key for local EIP-3009 signing.
	PrivateKey string

	// TEE configures TEE-based signing. Not yet implemented (Phase 2).
	TEE *TEEConfig
}

// NewOKXSigner creates a ClientEvmSigner based on the provided config.
// Phase 1: only private key signing is supported.
// Phase 2: TEE signing will be added with automatic fallback.
func NewOKXSigner(config OKXSignerConfig) (x402evm.ClientEvmSigner, error) {
	if config.TEE == nil && config.PrivateKey == "" {
		return nil, errors.New("must provide either tee or privateKey")
	}

	if config.TEE != nil {
		return nil, errors.New("TEE signing not yet implemented (Phase 2)")
	}

	return NewClientSignerFromPrivateKey(config.PrivateKey)
}
