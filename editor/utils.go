package editor

// Min of 2 int
func Min(x, y int) int {
	if x < y {
		return x
	}
	return y
}

// Max of 2 int
func Max(x, y int) int {
	if x > y {
		return x
	}
	return y
}

// Abs of x
func Abs(x int) int {
	if x < 0 {
		return -x
	}
	return x
}
