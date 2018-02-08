package editor

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

// TestFrameCount tests
func TestFrameCount(t *testing.T) {
	frame := &Frame{}
	frame.split(true)
	frame.f1.split(true)
	frame.f2.split(true)
	assert.Equal(t, 4, frame.countSplits(true))
	assert.Equal(t, frame, frame.f1.vTop)
	assert.Equal(t, frame, frame.f1.f2.vTop)

	frame = &Frame{}
	frame.split(false)
	frame.f1.split(true)
	frame.f2.split(true)
	frame.f2.f1.split(true)
	assert.Equal(t, 3, frame.f2.countSplits(true))
	assert.Equal(t, 2, frame.f1.countSplits(true))
	assert.Equal(t, frame.f1, frame.f1.f1.vTop)
	assert.Equal(t, frame.f2, frame.f2.f1.f1.vTop)
	assert.Equal(t, frame, frame.f1.f1.hTop)
	assert.Equal(t, frame, frame.f2.f1.f1.hTop)

	frame = &Frame{width: 8, height: 2}
	frame.split(true)
	frame.f1.split(false)
	frame.f1.f1.split(true)
	frame.f1.f1.f1.split(true)
	frame.f1.f2.split(true)
	assert.Equal(t, 2, frame.f2.width)
	assert.Equal(t, 3, frame.f1.f2.f1.width)
	assert.Equal(t, 3, frame.f1.f2.f2.width)
	assert.Equal(t, 2, frame.f1.f1.f1.f1.width)
	assert.Equal(t, 2, frame.f1.f1.f1.f2.width)
	assert.Equal(t, 2, frame.f1.f1.f2.width)

	frame.f2.close()
	frame.f1.f1.f1.f1.split(true)
	assert.Equal(t, 4, frame.f1.f2.f1.width)
	assert.Equal(t, 4, frame.f1.f2.f2.width)
	assert.Equal(t, 2, frame.f1.f1.f1.f1.width)
	assert.Equal(t, 2, frame.f1.f1.f1.f2.width)
	assert.Equal(t, 2, frame.f1.f1.f2.width)
}
