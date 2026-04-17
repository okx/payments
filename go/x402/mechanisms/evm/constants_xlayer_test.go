package evm

import (
	"strings"
	"testing"
)

func TestXLayerAssetRegistry_OutOfBox(t *testing.T) {
	config, ok := NetworkConfigs["eip155:196"]
	if !ok {
		t.Fatal("eip155:196 not found in NetworkConfigs — X Layer USDT should be registered out of box")
	}
	if config.DefaultAsset.Address == "" {
		t.Fatal("eip155:196 has no default asset configured")
	}
}

func TestXLayerAssetRegistry_UnicodeTokenName(t *testing.T) {
	config := NetworkConfigs["eip155:196"]
	name := config.DefaultAsset.Name

	// Must contain the Unicode tugrik sign ₮ (U+20AE)
	if !strings.Contains(name, "\u20AE") {
		t.Errorf("expected asset name to contain Unicode ₮ (U+20AE), got %q", name)
	}
	// Must be exactly "USD₮0"
	if name != "USD\u20AE0" {
		t.Errorf("expected asset name 'USD₮0', got %q", name)
	}
}

func TestXLayerAssetRegistry_ContractAndDecimals(t *testing.T) {
	config := NetworkConfigs["eip155:196"]

	expectedAddr := "0x779Ded0c9e1022225f8E0630b35a9b54bE713736"
	if !strings.EqualFold(config.DefaultAsset.Address, expectedAddr) {
		t.Errorf("expected contract address %s, got %s", expectedAddr, config.DefaultAsset.Address)
	}

	if config.DefaultAsset.Decimals != 6 {
		t.Errorf("expected decimals 6, got %d", config.DefaultAsset.Decimals)
	}
}

func TestXLayerAssetRegistry_ChainID(t *testing.T) {
	config := NetworkConfigs["eip155:196"]

	if config.ChainID == nil || config.ChainID.Int64() != 196 {
		t.Errorf("expected chainID 196, got %v", config.ChainID)
	}
}
