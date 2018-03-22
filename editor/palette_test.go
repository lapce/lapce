package editor

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestMatchScor(t *testing.T) {
	text := []rune("Split Left")
	pattern := []rune("sl")
	score, matches := matchScore(text, pattern)

	assert.Equal(t, 1, score)
	assert.Equal(t, []int{0, 6}, matches)

	text = []rune("Split Previous")
	pattern = []rune("sp")
	score, matches = matchScore(text, pattern)

	assert.Equal(t, 0, score)
	assert.Equal(t, []int{0, 1}, matches)

	text = []rune("1 // Previous")
	pattern = []rune("1")
	score, matches = matchScore(text, pattern)

	assert.Equal(t, 0, score)
	assert.Equal(t, []int{0}, matches)
}
