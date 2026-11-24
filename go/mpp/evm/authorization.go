package evm

import (
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
)

var validAuthTypes = map[string]bool{
	"SettleAuthorization": true,
	"CloseAuthorization":  true,
}

type SignedAuthorization struct {
	ChannelID        [32]byte
	CumulativeAmount *big.Int
	Nonce            *big.Int
	Deadline         *big.Int
	Signature        []byte // 65-byte r||s||v
}

func authorizationTypedData(
	primaryType string,
	channelID [32]byte,
	cumulativeAmount *big.Int,
	nonce *big.Int,
	deadline *big.Int,
	escrowContract common.Address,
	chainID uint64,
	domainName, domainVersion string,
) (apitypes.TypedData, error) {
	if !validAuthTypes[primaryType] {
		return apitypes.TypedData{}, fmt.Errorf("invalid authorization type: %q", primaryType)
	}
	if domainName == "" {
		domainName = DefaultDomainName
	}
	if domainVersion == "" {
		domainVersion = DefaultDomainVersion
	}

	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": {
				{Name: "name", Type: "string"},
				{Name: "version", Type: "string"},
				{Name: "chainId", Type: "uint256"},
				{Name: "verifyingContract", Type: "address"},
			},
			primaryType: {
				{Name: "channelId", Type: "bytes32"},
				{Name: "cumulativeAmount", Type: "uint128"},
				{Name: "nonce", Type: "uint256"},
				{Name: "deadline", Type: "uint256"},
			},
		},
		PrimaryType: primaryType,
		Domain: apitypes.TypedDataDomain{
			Name:              domainName,
			Version:           domainVersion,
			ChainId:           (*math.HexOrDecimal256)(new(big.Int).SetUint64(chainID)),
			VerifyingContract: escrowContract.Hex(),
		},
		Message: apitypes.TypedDataMessage{
			"channelId":        channelID[:],
			"cumulativeAmount": cumulativeAmount,
			"nonce":            nonce,
			"deadline":         deadline,
		},
	}, nil
}

// SignAuthorization signs a settle or close authorization.
// domainName and domainVersion default to DefaultDomainName / DefaultDomainVersion when empty.
func SignAuthorization(
	signer Signer,
	primaryType string,
	channelID [32]byte,
	cumulativeAmount *big.Int,
	nonce *big.Int,
	deadline *big.Int,
	escrowContract common.Address,
	chainID uint64,
	domainName string, 
	domainVersion string,
) (*SignedAuthorization, error) {
	td, err := authorizationTypedData(primaryType, channelID, cumulativeAmount, nonce, deadline, escrowContract, chainID, domainName, domainVersion)
	if err != nil {
		return nil, err
	}

	sig, err := signer.SignTypedData(td)
	if err != nil {
		return nil, fmt.Errorf("sign %s: %w", primaryType, err)
	}

	return &SignedAuthorization{
		ChannelID:        channelID,
		CumulativeAmount: new(big.Int).Set(cumulativeAmount),
		Nonce:            new(big.Int).Set(nonce),
		Deadline:         new(big.Int).Set(deadline),
		Signature:        sig,
	}, nil
}
