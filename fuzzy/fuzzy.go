package fuzzy

import (
	"unicode"

	"github.com/crane-editor/crane/utils"
)

// MatchScore gets the score
func MatchScore(text []rune, pattern []rune) (int, []int) {
	matches := []int{}

	start := 0
	s := 0
	for {
		score, index, n := matchContinuous(text, pattern, start)
		// fmt.Println(string(text), string(pattern), start, score, index, n)
		if score < 0 {
			return -1, nil
		}
		s += score
		for i := 0; i < n; i++ {
			matches = append(matches, index+i)
		}
		if n == len(pattern) {
			return s, matches
		}
		pattern = pattern[n:]
		start = index + n
	}
	return s, matches
}

func matchContinuous(text []rune, pattern []rune, start int) (int, int, int) {
	score := -1
	index := -1
	n := 1
	for {
		newPattern := pattern[:n]
		newScore := -1
		newIndex := -1
		if len(newPattern) == 1 {
			newScore, newIndex = bestMatch(text, start, newPattern[0])
		} else {
			newScore, newIndex = patternIndex(text, newPattern, start)
		}
		if newScore < 0 {
			return score, index, n - 1
		}
		score = newScore
		index = newIndex
		n++
		if n > len(pattern) {
			return score, index, n - 1
		}
	}
}

func bestMatch(text []rune, start int, r rune) (int, int) {
	class := 0
	s := 0
	for i := start; i < len(text); i++ {
		c := unicode.ToLower(text[i])
		if c == r || text[i] == r {
			if i == start {
				return 0, i
			}
			if utils.UtfClass(text[i-1]) != utils.UtfClass(r) {
				return i - start, i
			}
		} else {
			if i == start {
				class = utils.UtfClass(text[i])
			} else {
				newClass := utils.UtfClass(text[i])
				if newClass != class {
					s++
					class = newClass
				}
			}
		}
	}
	for i := start; i < len(text); i++ {
		c := unicode.ToLower(text[i])
		if c == r || text[i] == r {
			return (i - start) * 100, i
		}
	}
	return -1, -1
}

func patternIndex(text []rune, pattern []rune, start int) (int, int) {
	s := 0
	class := 0
	for i := start; i < len(text); i++ {
		if i == start {
			if patternMatch(text[i:], pattern) {
				return i - start, i
			}
			class = utils.UtfClass(text[i])
		} else {
			newClass := utils.UtfClass(text[i])
			if newClass != class {
				class = newClass
				s++
				if patternMatch(text[i:], pattern) {
					return i - start, i
				}
			}
		}
	}
	return -1, -1
}

func patternMatch(text []rune, pattern []rune) bool {
	if len(pattern) > len(text) {
		return false
	}
	for i, r := range pattern {
		c := unicode.ToLower(text[i])
		if c != r && text[i] != r {
			return false
		}
	}
	return true
}
