package utils

import "unicode"

// UtfClass of rune
func UtfClass(r rune) int {
	if unicode.IsSpace(r) {
		return 0
	}
	if unicode.IsPunct(r) || unicode.IsMark(r) || unicode.IsSymbol(r) {
		return 1
	}
	return 2
}
