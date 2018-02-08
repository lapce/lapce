package main

import "github.com/dzhou121/xi-go/editor"

func main() {
	editor, err := editor.NewEditor()
	if err != nil {
		return
	}
	editor.Run()
}
