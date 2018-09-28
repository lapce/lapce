package main

import (
	"net/http"

	_ "net/http/pprof"

	"github.com/crane-editor/crane/editor"
)

func main() {
	go func() {
		http.ListenAndServe("localhost:6020", nil)
	}()
	editor, err := editor.NewEditor()
	if err != nil {
		return
	}
	editor.Run()
}
