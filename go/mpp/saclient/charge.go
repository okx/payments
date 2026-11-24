package saclient

import (
	"context"
	"encoding/json"

	mpperrors "github.com/okx/payments/go/mpp/errors"
)

const (
	pathChargeSettle     = "/api/v6/pay/mpp/charge/settle"
	pathChargeVerifyHash = "/api/v6/pay/mpp/charge/verifyHash"
)

// Settle submits a charge for on-chain settlement via the SA API.
// POST /api/v6/pay/mpp/charge/settle
func (c *OKXSAClient) Settle(ctx context.Context, req *ChargeSettleRequest) (*ChargeReceipt, error) {
	resp, err := c.doPost(ctx, pathChargeSettle, req)
	if err != nil {
		return nil, err
	}
	return decodeData[ChargeReceipt](resp)
}

// VerifyHash verifies a client-broadcast transaction hash via the SA API.
// POST /api/v6/pay/mpp/charge/verifyHash
func (c *OKXSAClient) VerifyHash(ctx context.Context, req *ChargeVerifyHashRequest) (*ChargeReceipt, error) {
	resp, err := c.doPost(ctx, pathChargeVerifyHash, req)
	if err != nil {
		return nil, err
	}
	return decodeData[ChargeReceipt](resp)
}

// decodeData unmarshals SAResponse.Data into T.
func decodeData[T any](resp *SAResponse) (*T, error) {
	var out T
	if err := json.Unmarshal(resp.Data, &out); err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeJson,
			Message: "failed to decode SA API response data",
			Reason:  err.Error(),
		}
	}
	return &out, nil
}
