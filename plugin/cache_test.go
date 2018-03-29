package plugin

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestSetRaw(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nTest\nTest"))

	assert.Equal(t, "Test\n", l.Lines[0].Text)
	assert.Equal(t, "Test\n", l.Lines[1].Text)
	assert.Equal(t, "Test", l.Lines[2].Text)
}

func TestGetPos(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nTest\nTest"))

	row, col := l.GetPos(4)
	assert.Equal(t, 0, row)
	assert.Equal(t, 4, col)

	row, col = l.GetPos(5)
	assert.Equal(t, 1, row)
	assert.Equal(t, 0, col)
}

func TestApplyUpdate(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nSecond\nThird"))

	var update Update
	el := struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{0, 3},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	el = struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Insert: "\ninside\n",
	}
	update.Delta.Els = append(update.Delta.Els, el)
	el = struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{3, 14},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	l.ApplyUpdate(&update)
	assert.Equal(t, "Tes\ninside\nt\nSecond\nThird", string(l.Raw))
	assert.Equal(t, "Tes\n", l.Lines[0].Text)
	assert.Equal(t, "inside\n", l.Lines[1].Text)
	assert.Equal(t, "t\n", l.Lines[2].Text)
	assert.Equal(t, "Second\n", l.Lines[3].Text)
	assert.Equal(t, "Third", l.Lines[4].Text)
}

func TestApplyUpdateDelete(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nSecond\nThird"))

	var update Update
	el := struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{0, 3},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	el = struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{4, 14},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	l.ApplyUpdate(&update)
	assert.Equal(t, "Tes\nSecond\nThird", string(l.Raw))
	assert.Equal(t, "Tes\n", l.Lines[0].Text)
	assert.Equal(t, "Second\n", l.Lines[1].Text)
	assert.Equal(t, "Third", l.Lines[2].Text)
}

func TestApplyUpdateDeleteNewLine(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nSecond\nThird"))

	var update Update
	el := struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{0, 4},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	el = struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{5, 14},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	l.ApplyUpdate(&update)
	assert.Equal(t, "TestSecond\nThird", string(l.Raw))
	assert.Equal(t, "TestSecond\n", l.Lines[0].Text)
	assert.Equal(t, "Third", l.Lines[1].Text)
}

func TestApplyUpdateDeleteInLastLine(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nSecond\nThird"))

	var update Update
	el := struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{0, 11},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	el = struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{12, 14},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	l.ApplyUpdate(&update)
	assert.Equal(t, "Test\nSecondThird", string(l.Raw))
	assert.Equal(t, "Test\n", l.Lines[0].Text)
	assert.Equal(t, "SecondThird", l.Lines[1].Text)
}

func TestApplyUpdateDeleteLastChar(t *testing.T) {
	l := LineCache{}
	l.SetRaw([]byte("Test\nSecond\nThird"))

	var update Update
	el := struct {
		Copy   []int  `json:"copy,omitempty"`
		Insert string `json:"insert,omitempty"`
	}{
		Copy: []int{0, 4},
	}
	update.Delta.Els = append(update.Delta.Els, el)
	l.ApplyUpdate(&update)
	assert.Equal(t, "Test", string(l.Raw))
	assert.Equal(t, "Test", l.Lines[0].Text)
}
