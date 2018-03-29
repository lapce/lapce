package lsp

import (
	"io/ioutil"
	"log"
	"os"
	"runtime/debug"

	"github.com/dzhou121/crane/plugin"
)

// Plugin is
type Plugin struct {
	plugin *plugin.Plugin
	lsp    map[string]*Client
	views  map[string]*plugin.View
}

// NewPlugin is
func NewPlugin() *Plugin {
	p := &Plugin{
		plugin: plugin.NewPlugin(),
		lsp:    map[string]*Client{},
		views:  map[string]*plugin.View{},
	}
	p.plugin.SetHandleFunc(p.handle)
	return p
}

// Run is
func (p *Plugin) Run() {
	file, err := os.OpenFile("/tmp/log", os.O_APPEND|os.O_WRONLY, 0666)
	if err != nil {
		log.Fatal(err)
	}
	log.SetOutput(file)
	log.Println("now start to run")
	<-p.plugin.Stop
}

func (p *Plugin) handle(req interface{}) {
	defer func() {
		if r := recover(); r != nil {
			log.Println("handle error", r, string(debug.Stack()))
		}
	}()
	switch r := req.(type) {
	case *plugin.Initialization:
		for _, buf := range r.BufferInfo {
			viewID := buf.Views[0]
			view := &plugin.View{
				ID:     viewID,
				Path:   buf.Path,
				Syntax: buf.Syntax,
				LineCache: &plugin.LineCache{
					ViewID: viewID,
				},
			}
			p.views[viewID] = view
			lspClient, ok := p.lsp[buf.Syntax]
			if !ok {
				var err error
				lspClient, err = NewClient()
				if err != nil {
					return
				}
				dir, err := os.Getwd()
				if err != nil {
					return
				}
				err = lspClient.Initialize(dir)
				if err != nil {
					return
				}
				p.lsp[buf.Syntax] = lspClient
			}

			content, err := ioutil.ReadFile(buf.Path)
			if err != nil {
				return
			}
			log.Println("now set raw content")
			view.LineCache.SetRaw(content)
			log.Println("set raw content done", buf.Path)
			err = lspClient.DidOpen(buf.Path, string(content))
			log.Println("did open done")
			if err != nil {
				return
			}
		}
	case *plugin.Update:
		view := p.views[r.ViewID]
		startRow, startCol, endRow, endCol, text := view.LineCache.ApplyUpdate(r)
		didChange := &DidChangeParams{
			TextDocument: VersionedTextDocumentIdentifier{
				URI: "file://" + view.Path,
			},
			ContentChanges: []*ContentChange{
				&ContentChange{
					Range: &Range{
						Start: &Position{
							Line:      startRow,
							Character: startCol,
						},
						End: &Position{
							Line:      endRow,
							Character: endCol,
						},
					},
					Text: text,
				},
			},
		}
		lspClient := p.lsp[view.Syntax]
		lspClient.DidChange(didChange)
	}
}
