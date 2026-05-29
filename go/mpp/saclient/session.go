package saclient

import "context"

const (
	pathSessionOpen   = "/api/v6/pay/mpp/session/open"
	pathSessionTopUp  = "/api/v6/pay/mpp/session/topUp"
	pathSessionSettle = "/api/v6/pay/mpp/session/settle"
	pathSessionClose  = "/api/v6/pay/mpp/session/close"
	pathSessionStatus = "/api/v6/pay/mpp/session/status"
)

// SessionOpen creates a new payment channel session.
// POST /api/v6/pay/mpp/session/open
func (c *OKXSAClient) SessionOpen(ctx context.Context, req *SessionOpenRequest) (*SessionReceipt, error) {
	resp, err := c.doPost(ctx, pathSessionOpen, req)
	if err != nil {
		return nil, err
	}
	return decodeData[SessionReceipt](resp)
}

// SessionTopUp adds deposit to an existing payment channel.
// POST /api/v6/pay/mpp/session/topUp
func (c *OKXSAClient) SessionTopUp(ctx context.Context, req *SessionTopUpRequest) (*SessionReceipt, error) {
	resp, err := c.doPost(ctx, pathSessionTopUp, req)
	if err != nil {
		return nil, err
	}
	return decodeData[SessionReceipt](resp)
}

// SessionSettle triggers an intermediate on-chain settlement for a channel.
// POST /api/v6/pay/mpp/session/settle
func (c *OKXSAClient) SessionSettle(ctx context.Context, req *SessionSettleRequest) (*SessionReceipt, error) {
	resp, err := c.doPost(ctx, pathSessionSettle, req)
	if err != nil {
		return nil, err
	}
	return decodeData[SessionReceipt](resp)
}

// SessionClose finalizes and closes a payment channel.
// POST /api/v6/pay/mpp/session/close
func (c *OKXSAClient) SessionClose(ctx context.Context, req *SessionCloseRequest) (*SessionReceipt, error) {
	resp, err := c.doPost(ctx, pathSessionClose, req)
	if err != nil {
		return nil, err
	}
	return decodeData[SessionReceipt](resp)
}

// SessionStatus queries the current state of a payment channel.
// GET /api/v6/pay/mpp/session/status?channelId=...
func (c *OKXSAClient) SessionStatus(ctx context.Context, channelID string) (*SessionStatus, error) {
	resp, err := c.doGet(ctx, pathSessionStatus, map[string]string{"channelId": channelID})
	if err != nil {
		return nil, err
	}
	return decodeData[SessionStatus](resp)
}
