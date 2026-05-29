package evm

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestNewOKXSigner_WithPrivateKey(t *testing.T) {
	// Use a well-known test private key (do NOT use in production)
	privateKey := "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

	signer, err := NewOKXSigner(OKXSignerConfig{
		PrivateKey: privateKey,
	})
	require.NoError(t, err)
	require.NotNil(t, signer)

	// Should derive the correct address from the private key
	assert.Equal(t, "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", signer.Address())
}

func TestNewOKXSigner_NoConfig(t *testing.T) {
	_, err := NewOKXSigner(OKXSignerConfig{})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "must provide either tee or privateKey")
}

func TestNewOKXSigner_TEENotImplemented(t *testing.T) {
	_, err := NewOKXSigner(OKXSignerConfig{
		TEE: &TEEConfig{
			Endpoint:  "https://tee.example.com",
			AccessKey: "ak-123",
		},
	})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "TEE signing not yet implemented")
}
