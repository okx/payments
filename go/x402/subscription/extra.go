package subscription

import "encoding/json"

// Helpers for reading loosely-typed payment-requirements extra. Values may be
// native Go integers (freshly built offers) or float64 (JSON round-tripped), so
// the numeric getters accept both.

func extraStr(extra map[string]interface{}, key string) (string, bool) {
	v, ok := extra[key]
	if !ok {
		return "", false
	}
	s, ok := v.(string)
	return s, ok
}

func asUint64(v interface{}) (uint64, bool) {
	switch n := v.(type) {
	case float64:
		return uint64(n), true
	case int:
		return uint64(n), true
	case int64:
		return uint64(n), true
	case uint:
		return uint64(n), true
	case uint32:
		return uint64(n), true
	case uint64:
		return n, true
	case uint8:
		return uint64(n), true
	case json.Number:
		i, err := n.Int64()
		if err != nil {
			return 0, false
		}
		return uint64(i), true
	default:
		return 0, false
	}
}

func extraUint64(extra map[string]interface{}, key string) (uint64, bool) {
	v, ok := extra[key]
	if !ok {
		return 0, false
	}
	return asUint64(v)
}

// planFields reads the plan sub-object's id and tier from an accept's extra.
func planFields(extra map[string]interface{}) (id string, tier uint8) {
	plan, ok := extra["plan"].(map[string]interface{})
	if !ok {
		return "", 0
	}
	if s, ok := plan["id"].(string); ok {
		id = s
	}
	if t, ok := asUint64(plan["tier"]); ok {
		tier = uint8(t)
	}
	return id, tier
}
