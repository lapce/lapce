package main

import (
	"net/http"
	"os"

	_ "net/http/pprof"

	"github.com/crane-editor/crane/editor"
	"github.com/crane-editor/crane/log"
)

func main() {
	os.Setenv("PATH", "/Users/Lulu/.cargo/bin:/Users/Lulu/go/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/local/bin:/opt/local/sbin")
	go func() {
		http.ListenAndServe("localhost:6020", nil)
	}()
	file, err := os.OpenFile("/Users/Lulu/.crane/log", os.O_APPEND|os.O_WRONLY|os.O_CREATE, 0666)
	if err != nil {
		log.Fatal(err)
	}
	log.Base().SetOutput(file)
	editor, err := editor.NewEditor()
	if err != nil {
		log.Errorln(err)
		return
	}
	log.Infoln("now run editor")
	editor.Run()
}
