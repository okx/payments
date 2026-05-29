package evm

import (
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/google/uuid"
)

type NonceProvider interface {
	Allocate(payee common.Address, channelID [32]byte) (*big.Int, error)
}

type UuidNonceProvider struct{}

func NewUuidNonceProvider() *UuidNonceProvider {
	return &UuidNonceProvider{}
}

func (p *UuidNonceProvider) Allocate(_ common.Address, _ [32]byte) (*big.Int, error) {
	id, err := uuid.NewRandom()
	if err != nil {
		return nil, fmt.Errorf("generate uuid: %w", err)
	}
	return new(big.Int).SetBytes(id[:]), nil
}
