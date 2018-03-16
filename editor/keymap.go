package editor

import (
	"path/filepath"

	"github.com/BurntSushi/toml"
	homedir "github.com/mitchellh/go-homedir"
)

// Keymap is
type Keymap struct {
	editor *Editor
	Normal map[string]string
	Insert map[string]string
}

func loadKeymap(e *Editor) {
	e.keymap = &Keymap{
		editor: e,
		Normal: map[string]string{},
		Insert: map[string]string{},
	}
	home, err := homedir.Dir()
	if err != nil {
		return
	}
	path := filepath.Join(home, ".crane", "keymap.toml")
	_, err = toml.DecodeFile(path, e.keymap)
}

func (k *Keymap) lookup(input string) []string {
	var keysMap map[string]string
	switch k.editor.mode {
	case Insert:
		keysMap = k.Insert
	case Normal:
		keysMap = k.Normal
	default:
		keysMap = map[string]string{}
	}
	key, ok := keysMap[input]
	if !ok {
		return nil
	}
	special := false
	specialKey := ""
	keys := []string{}
	for _, c := range key {
		if c == '<' {
			special = true
			specialKey += "<"
		} else if c == '>' {
			if special {
				specialKey += ">"
				keys = append(keys, specialKey)
				special = false
				specialKey = ""
			} else {
				keys = append(keys, string(c))
			}
		} else {
			if special {
				specialKey += string(c)
			} else {
				keys = append(keys, string(c))
			}
		}
	}
	return keys
}
