package editor

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestMatchScore(t *testing.T) {
	text := []rune(" Ser flasdj")
	pattern := []rune("self")
	score, matches := matchScore(text, pattern)

	assert.Equal(t, 1, score)
	assert.Equal(t, []int{1, 2, 3}, matches)
}

// func TestPatternIndex(t *testing.T) {
// 	text := []rune("Laeft Left left")
// 	pattern := []rune("left")

// 	assert.Equal(t, false, patternMatch(text, pattern))
// 	assert.Equal(t, 2, patternIndex(text, pattern, 0))
// 	assert.Equal(t, 2, matchContinuous(text, pattern, 0))

// 	text = []rune("Laeft Lef left")
// 	pattern = []rune("left")
// 	assert.Equal(t, 4, patternIndex(text, pattern, 0))
// 	assert.Equal(t, 4, matchContinuous(text, pattern, 0))

// 	text = []rune("Laeft Lef ljsdlfkj")
// 	pattern = []rune("left")
// 	assert.Equal(t, 2, matchContinuous(text, pattern, 0))
// }
