package plugin

import (
	"context"
	"encoding/json"
	"io/ioutil"
	"os"
	"runtime/debug"
	"sync"

	"github.com/crane-editor/crane/log"

	"github.com/crane-editor/crane/fuzzy"
	"github.com/crane-editor/crane/lsp"
	"github.com/crane-editor/crane/plugin"
	"github.com/crane-editor/crane/utils"
	"github.com/sourcegraph/jsonrpc2"
)

// Plugin is
type Plugin struct {
	*plugin.Plugin
	lsp             map[string]*lsp.Client
	lspMutex        sync.Mutex
	conns           map[string]*jsonrpc2.Conn
	server          *Server
	completionItems []*lsp.CompletionItem
	completionShown bool
}

// NewPlugin is
func NewPlugin() *Plugin {
	p := &Plugin{
		Plugin: plugin.NewPlugin(),
		lsp:    map[string]*lsp.Client{},
		conns:  map[string]*jsonrpc2.Conn{},
	}
	p.SetHandleBeforeFunc(p.handleBefore)
	p.SetHandleFunc(p.handle)
	return p
}

// Run is
func (p *Plugin) Run() {
	file, err := os.OpenFile("/tmp/log", os.O_APPEND|os.O_WRONLY, 0666)
	if err != nil {
		log.Fatal(err)
	}
	log.Base().SetOutput(file)
	log.Infoln("now start to run")
	go func() {
		server, err := newServer(p)
		if err != nil {
			return
		}
		server.run()
	}()
	<-p.Stop
}

func (p *Plugin) handleNotification(notification interface{}) {
	switch n := notification.(type) {
	case *lsp.PublishDiagnosticsParams:
		for _, conn := range p.conns {
			conn.Notify(context.Background(), "diagnostics", n)
		}
	}
}

func (p *Plugin) handleBefore(req interface{}) (result interface{}, overide bool) {
	defer func() {
		if r := recover(); r != nil {
			log.Infoln("handle error", r, string(debug.Stack()))
		}
	}()
	switch r := req.(type) {
	case *plugin.Update:
		overide = true
		view, ok := p.Views[r.ViewID]
		if !ok {
			return
		}
		ver := int(view.Rev)
		didChange := &lsp.DidChangeParams{
			TextDocument: lsp.VersionedTextDocumentIdentifier{
				URI:     "file://" + view.Path,
				Version: &ver,
			},
			ContentChanges: []*lsp.ContentChange{
				&lsp.ContentChange{},
			},
		}

		change := didChange.ContentChanges[0]
		full := false
		lspClient := p.lsp[view.Syntax]
		startRow := 0
		startCol := 0
		newText := ""
		deletedText := ""
		if r.IsSimpleInsert() {
			els := r.Delta.Els
			startRow, startCol = view.Cache.OffsetToPos(els[0].Copy[1])
			endRow, endCol := view.Cache.OffsetToPos(els[2].Copy[0])
			change.Range = &lsp.Range{
				Start: &lsp.Position{
					Line:      startRow,
					Character: startCol,
				},
				End: &lsp.Position{
					Line:      endRow,
					Character: endCol,
				},
			}
			change.Text = els[1].Insert
			newText = els[1].Insert
		} else if r.IsSimpleDelete() {
			els := r.Delta.Els
			startRow, startCol = view.Cache.OffsetToPos(els[0].Copy[1])
			endRow, endCol := view.Cache.OffsetToPos(els[1].Copy[0])
			change.Range = &lsp.Range{
				Start: &lsp.Position{
					Line:      startRow,
					Character: startCol,
				},
				End: &lsp.Position{
					Line:      endRow,
					Character: endCol,
				},
			}
			deletedText = string(view.Cache.GetChunk(els[0].Copy[1], els[1].Copy[0]))
		} else {
			full = true
		}
		view.Cache.ApplyUpdate(r)
		if lspClient.ServerCapabilities.TextDocumentSync == 1 {
			full = true
		}
		if full {
			change.Range = nil
			change.RangeLength = nil
			change.Text = string(view.Cache.GetContent())
		}
		bytes, _ := json.Marshal(didChange)
		log.Infoln("send did change")
		log.Infoln(string(bytes))
		lspClient.DidChange(didChange)
		p.complete(lspClient, view, newText, deletedText, startRow, startCol)
	}
	return
}

func (p *Plugin) handle(req interface{}) (result interface{}, overide bool) {
	defer func() {
		if r := recover(); r != nil {
			log.Infoln("handle error", r, string(debug.Stack()))
		}
	}()
	switch r := req.(type) {
	case *plugin.Initialization:
		for _, buf := range r.BufferInfo {
			viewID := buf.Views[0]
			view := p.Views[viewID]
			syntax := view.Syntax
			log.Infoln("syntax is", syntax)
			p.lspMutex.Lock()
			lspClient, ok := p.lsp[syntax]
			if !ok {
				log.Infoln("create lspClient")
				var err error
				lspClient, err = lsp.NewClient(syntax, p.handleNotification)
				if err != nil {
					log.Infoln("err new lsp client", err, "sytax is", syntax)
					return
				}
				dir, err := os.Getwd()
				if err != nil {
					log.Infoln("Getwd error", err, syntax)
					return
				}
				err = lspClient.Initialize(dir)
				if err != nil {
					log.Infoln("Initialize err", err, dir, syntax)
					return
				}
				p.lsp[syntax] = lspClient
			}
			p.lspMutex.Unlock()

			content, err := ioutil.ReadFile(buf.Path)
			if err != nil {
				log.Infoln("err read file content", err)
				return
			}
			log.Infoln("now set raw content")
			view.Cache.SetContent(content)
			log.Infoln("set raw content done", buf.Path)
			err = lspClient.DidOpen(buf.Path, string(content))
			log.Infoln("did open done")
			if err != nil {
				return
			}
		}
	case *plugin.Update:
		// view := p.Views[r.ViewID]
		// sendFull := false
		// if len(r.Delta.Els[len(r.Delta.Els)-1].Copy) > 0 && r.NewLen != r.Delta.Els[len(r.Delta.Els)-1].Copy[1] {
		// 	sendFull = true
		// }
		// startRow, startCol, endRow, endCol, text, deletedText, changed := view.ApplyUpdate(r)
		// log.Infoln(startRow, startCol, endRow, endCol, text, deletedText, changed)
		// if !changed {
		// 	return 0
		// }
		// ver := int(view.Rev)
		// didChange := &lsp.DidChangeParams{
		// 	TextDocument: lsp.VersionedTextDocumentIdentifier{
		// 		URI:     "file://" + view.Path,
		// 		Version: &ver,
		// 	},
		// 	ContentChanges: []*lsp.ContentChange{
		// 		&lsp.ContentChange{
		// 			Range: &lsp.Range{
		// 				Start: &lsp.Position{
		// 					Line:      startRow,
		// 					Character: startCol,
		// 				},
		// 				End: &lsp.Position{
		// 					Line:      endRow,
		// 					Character: endCol,
		// 				},
		// 			},
		// 			Text: text,
		// 		},
		// 	},
		// }
		// if sendFull {
		// 	log.Infoln("send full")
		// 	didChange.ContentChanges[0].Range = nil
		// 	didChange.ContentChanges[0].Text = string(view.LineCache.Raw)
		// }
		// lspClient := p.lsp[view.Syntax]
		// if lspClient.ServerCapabilities.TextDocumentSync == 1 {
		// 	log.Infoln("full sync")
		// 	didChange.ContentChanges[0].Range = nil
		// 	didChange.ContentChanges[0].Text = string(view.LineCache.Raw)
		// }

		// bytes, _ := json.Marshal(r)
		// log.Infoln("plugin get update")
		// log.Infoln(string(bytes))
		// bytes, _ = json.Marshal(didChange)
		// log.Infoln("send did change")
		// log.Infoln(string(bytes))
		// lspClient.DidChange(didChange)
		// p.complete(lspClient, view, text, deletedText, startRow, startCol)
	}
	return
}

func (p *Plugin) signature(lspClient *lsp.Client, view *plugin.View, text string, deletedText string, startRow int, startCol int) {
	if text != "(" {
		return
	}
	pos := lsp.Position{
		Line:      startRow,
		Character: startCol + 1,
	}
	params := &lsp.TextDocumentPositionParams{
		TextDocument: lsp.TextDocumentIdentifier{
			URI: "file://" + view.Path,
		},
		Position: pos,
	}
	lspClient.Signature(params)
}

func (p *Plugin) complete(lspClient *lsp.Client, view *plugin.View, text string, deletedText string, startRow int, startCol int) {
	log.Infoln("new text is", text)
	log.Infoln("deleted text is", deletedText)
	runes := []rune(text)
	deletedRunes := []rune(deletedText)

	reset := false
	if len(runes) > 1 {
		reset = true
	}
	if !reset {
		for _, r := range runes {
			if utils.UtfClass(r) != 2 {
				reset = true
				break
			}
		}
	}
	if !reset {
		for _, r := range deletedRunes {
			if utils.UtfClass(r) != 2 {
				reset = true
				break
			}
		}
	}
	if reset && len(p.completionItems) > 0 {
		p.completionItems = []*lsp.CompletionItem{}
	}

	if len(runes) > 1 {
		p.notifyCompletion(p.completionItems)
		return
	}

	if len(runes) > 0 {
		lastRune := runes[len(runes)-1]
		if lastRune != '.' && utils.UtfClass(runes[len(runes)-1]) != 2 {
			p.notifyCompletion(p.completionItems)
			return
		}
	}

	items := p.getCompletionItems(lspClient, view, text, startRow, startCol)
	p.notifyCompletion(items)
}

func (p *Plugin) notifyCompletion(items []*lsp.CompletionItem) {
	if len(items) > 0 {
		p.completionShown = true
	} else {
		p.completionShown = false
	}
	for _, conn := range p.conns {
		conn.Notify(context.Background(), "completion", items)
	}
}

func (p *Plugin) notifyCompletionPos(pos *lsp.Position) {
	for _, conn := range p.conns {
		conn.Notify(context.Background(), "completion_pos", pos)
	}
}

func (p *Plugin) getCompletionItems(lspClient *lsp.Client, view *plugin.View, text string, startRow int, startCol int) []*lsp.CompletionItem {
	if len(p.completionItems) > 0 {
		if text == "" {
			startCol--
		}
		_, word := p.getWord(view, startRow, startCol)
		log.Infoln("word is", string(word))
		return p.matchCompletionItems(p.completionItems, word)
	}

	word := []rune{}
	if len(text) == 1 {
		if text == "." {
			startCol++
		} else if utils.UtfClass([]rune(text)[0]) == 2 {
			startCol, word = p.getWord(view, startRow, startCol)
		}
	} else if text == "" {
		// startCol, word = p.getWord(view, startRow, startCol-1)
		return p.completionItems
	}
	pos := lsp.Position{
		Line:      startRow,
		Character: startCol,
	}
	params := &lsp.TextDocumentPositionParams{
		TextDocument: lsp.TextDocumentIdentifier{
			URI: "file://" + view.Path,
		},
		Position: pos,
	}
	resp, err := lspClient.Completion(params)
	if err != nil {
		return []*lsp.CompletionItem{}
	}
	p.notifyCompletionPos(&pos)
	p.completionItems = resp.Items
	return p.matchCompletionItems(p.completionItems, word)
}

func (p *Plugin) matchCompletionItems(items []*lsp.CompletionItem, word []rune) []*lsp.CompletionItem {
	if len(word) == 0 {
		for _, item := range items {
			if len(item.Matches) > 0 {
				item.Matches = []int{}
			}
		}
		return items
	}
	matchItems := []*lsp.CompletionItem{}
	for _, item := range items {
		score, matches := fuzzy.MatchScore([]rune(item.InsertText), word)
		if score > -1 {
			i := 0
			for i = 0; i < len(matchItems); i++ {
				matchItem := matchItems[i]
				if score < matchItem.Score {
					break
				}
			}
			item.Score = score
			item.Matches = matches
			matchItems = append(matchItems, nil)
			copy(matchItems[i+1:], matchItems[i:])
			matchItems[i] = item
		}
	}
	return matchItems
}

func (p *Plugin) getWord(view *plugin.View, row, col int) (int, []rune) {
	line := view.Cache.GetLine(row)
	runes := []rune(string(line))
	word := []rune{}
	for i := col; i >= 0; i-- {
		if utils.UtfClass(runes[i]) != 2 {
			return i + 1, word
		}
		word = append([]rune{runes[i]}, word...)
	}
	return 0, word
}
