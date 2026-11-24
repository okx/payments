package subscription

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

type elapsedVector struct {
	Name            string `json:"name"`
	PeriodMode      uint8  `json:"periodMode"`
	StartAt         int64  `json:"startAt"`
	BillingAnchorAt int64  `json:"billingAnchorAt"`
	PeriodSec       int64  `json:"periodSec"`
	Now             int64  `json:"now"`
	Expected        int64  `json:"expected"`
}

func TestElapsedPeriodsVectors(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "elapsed_periods.json"))
	require.NoError(t, err)

	var vectors []elapsedVector
	require.NoError(t, json.Unmarshal(raw, &vectors))
	require.NotEmpty(t, vectors)

	for _, v := range vectors {
		t.Run(v.Name, func(t *testing.T) {
			got := ElapsedPeriods(v.PeriodMode, v.StartAt, v.BillingAnchorAt, v.PeriodSec, v.Now)
			assert.Equal(t, v.Expected, got)
		})
	}
}

func TestAddCalendarMonthsMonthEndTruncation(t *testing.T) {
	jan31 := int64(1612051200) // 2021-01-31 00:00:00 UTC
	cases := []struct {
		months int
		want   int64
	}{
		{1, 1614470400}, // Feb 28 2021
		{2, 1617148800}, // Mar 31 2021
		{3, 1619740800}, // Apr 30 2021
	}
	for _, c := range cases {
		assert.Equal(t, c.want, AddCalendarMonths(jan31, c.months))
	}
}

func TestAddCalendarMonthsLeapYear(t *testing.T) {
	jan31Leap := int64(1580428800) // 2020-01-31
	feb29 := int64(1582934400)     // 2020-02-29
	assert.Equal(t, feb29, AddCalendarMonths(jan31Leap, 1))
}

func TestElapsedPeriodsUnclamped(t *testing.T) {
	// 13 periods elapsed with maxPeriods=12 is returned un-clamped.
	got := ElapsedPeriods(PeriodModeFixed, 0, 0, 100, 1250)
	assert.Equal(t, int64(13), got)
}
