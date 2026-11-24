package protocol

// Receipt represents the data in a Payment-Receipt header.
type Receipt struct {
	ID         string        `json:"id"`
	Status     ReceiptStatus `json:"status"`
	Method     MethodName    `json:"method"`
	Intent     IntentName    `json:"intent"`
	Settlement string        `json:"settlement"`
}

// SessionVerifyResult is the result of session verification, including
// an optional management response. Mirrors mpp-rs SessionVerifyResult.
type SessionVerifyResult struct {
	// Receipt is the payment receipt.
	Receipt *Receipt
	// ManagementResponse, when non-nil, indicates a management action
	// (open, topUp, close). The caller should return this as the response
	// body instead of proceeding with normal request handling.
	// When nil (voucher/replay), the caller proceeds to serve the resource.
	ManagementResponse any
}

// NewSuccessReceipt creates a Receipt with status "success".
func NewSuccessReceipt(id string, method MethodName, intent IntentName, settlement string) *Receipt {
	return &Receipt{
		ID:         id,
		Status:     ReceiptStatusSuccess,
		Method:     method,
		Intent:     intent,
		Settlement: settlement,
	}
}

// ToHeader formats the receipt as a Payment-Receipt header value.
func (r *Receipt) ToHeader() (string, error) {
	return FormatReceipt(r)
}
