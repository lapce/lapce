package main

import "github.com/dzhou121/crane/lsp"

func main() {
	plugin := lsp.NewPlugin()
	plugin.Run()
}
