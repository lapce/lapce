package plugin

// Delta is
type Delta struct {
	BaseLen int   `json:"base_len"`
	Els     []*El `json:"els"`
}

// El is
type El struct {
	Copy   []int  `json:"copy,omitempty"`
	Insert string `json:"insert,omitempty"`
}

// Update is
type Update struct {
	Author string `json:"author"`
	Delta  struct {
		BaseLen int `json:"base_len"`
		Els     []struct {
			Copy   []int  `json:"copy,omitempty"`
			Insert string `json:"insert,omitempty"`
		} `json:"els"`
	} `json:"delta"`
	EditType string `json:"edit_type"`
	NewLen   int    `json:"new_len"`
	Rev      uint64 `json:"rev"`
	ViewID   string `json:"view_id"`
}

// IsSimpleInsert checks
func (u *Update) IsSimpleInsert() bool {
	els := u.Delta.Els
	if len(els) != 3 {
		return false
	}
	if len(els[2].Copy) < 2 {
		return false
	}
	if len(els[0].Copy) < 2 {
		return false
	}
	if len(els[1].Copy) == 2 {
		return false
	}
	if els[2].Copy[1] != u.Delta.BaseLen {
		return false
	}
	if els[0].Copy[0] != 0 {
		return false
	}
	return true
}

// IsSimpleDelete checks
func (u *Update) IsSimpleDelete() bool {
	els := u.Delta.Els
	if len(els) != 2 {
		return false
	}
	if len(els[1].Copy) < 2 {
		return false
	}
	if len(els[0].Copy) < 2 {
		return false
	}
	if els[1].Copy[1] != u.Delta.BaseLen {
		return false
	}
	if els[0].Copy[0] != 0 {
		return false
	}
	return true
}
