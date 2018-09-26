package plugin

import (
	"context"
	"encoding/json"
	"net"
	"runtime/debug"

	"github.com/dzhou121/crane/log"

	"github.com/dzhou121/crane/lsp"
	"github.com/dzhou121/crane/plugin"
	"github.com/sourcegraph/jsonrpc2"
)

// Server is the rpc server for lsp plugin
type Server struct {
	lis    net.Listener
	plugin *Plugin
}

type handler struct {
	plugin *Plugin
}

func newServer(plugin *Plugin) (*Server, error) {
	log.Infoln("now listen")
	lis, err := net.Listen("tcp", "127.0.0.1:50051")
	if err != nil {
		log.Infoln("now listen", err)
		return nil, err
	}
	return &Server{
		lis:    lis,
		plugin: plugin,
	}, nil
}

func (s *Server) run() {
	for {
		conn, err := s.lis.Accept()
		if err != nil {
			return
		}
		go s.serve(conn)
	}
}

func (s *Server) serve(conn net.Conn) {
	log.Infoln("now serve", conn.RemoteAddr().String())
	s.plugin.conns[conn.RemoteAddr().String()] = jsonrpc2.NewConn(context.Background(), NewConnStream(conn), &handler{plugin: s.plugin})
}

func (h *handler) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	defer func() {
		if r := recover(); r != nil {
			log.Infoln("handle error", r, string(debug.Stack()))
		}
	}()

	paramsData, err := req.Params.MarshalJSON()
	if err != nil {
		log.Infoln(err)
		return
	}
	log.Infoln("now handle", req.ID, req.Method, string(paramsData))
	var meta map[string]string
	metaData, err := req.Meta.MarshalJSON()
	if err != nil {
		return
	}
	err = json.Unmarshal(metaData, &meta)
	if err != nil {
		return
	}
	viewID, ok := meta["view_id"]
	if !ok {
		return
	}
	log.Infoln("view id", viewID)
	switch req.Method {
	case "definition":
		var params *lsp.TextDocumentPositionParams
		err = json.Unmarshal(paramsData, &params)
		if err != nil {
			return
		}
		view, ok := h.plugin.views[viewID]
		if !ok {
			return
		}
		lspClient, ok := h.plugin.lsp[view.Syntax]
		if !ok {
			return
		}
		locations, err := lspClient.Definition(params)
		if err != nil {
			return
		}
		if len(locations) == 0 {
			return
		}
		for _, conn := range h.plugin.conns {
			conn.Notify(context.Background(), "definition", locations[0])
		}
	case "hover":
		var params *lsp.TextDocumentPositionParams
		err = json.Unmarshal(paramsData, &params)
		if err != nil {
			return
		}
		view, ok := h.plugin.views[viewID]
		if !ok {
			return
		}
		lspClient, ok := h.plugin.lsp[view.Syntax]
		if !ok {
			return
		}
		lspClient.Hover(params)
	case "completion":
		var params *lsp.TextDocumentPositionParams
		err = json.Unmarshal(paramsData, &params)
		if err != nil {
			return
		}
		view, ok := h.plugin.views[viewID]
		if !ok {
			return
		}
		lspClient, ok := h.plugin.lsp[view.Syntax]
		if !ok {
			return
		}
		resp, err := lspClient.Completion(params)
		if err != nil {
			return
		}
		log.Infoln("get resp", resp)
		conn.Reply(ctx, req.ID, resp)
		log.Infoln("resp replied")
	case "completion_reset":
		h.plugin.completionItems = []*lsp.CompletionItem{}
	case "completion_select":
		h.plugin.plugin.Mutex.Lock()
		defer h.plugin.plugin.Mutex.Unlock()

		var item *lsp.CompletionItem
		err = json.Unmarshal(paramsData, &item)
		if err != nil {
			return
		}
		view, ok := h.plugin.views[viewID]
		if !ok {
			return
		}
		start := item.TextEdit.Range.Start

		els := []*plugin.El{}
		el := &plugin.El{
			Copy: []int{0, view.GetOffset(start.Line, start.Character)},
		}
		els = append(els, el)
		el = &plugin.El{
			Insert: item.InsertText,
		}
		els = append(els, el)
		el = &plugin.El{
			Copy: []int{view.Offset, len(view.LineCache.Raw)},
		}
		els = append(els, el)
		delta := &plugin.Delta{
			BaseLen: len(view.LineCache.Raw),
			Els:     els,
		}
		edit := &plugin.Edit{
			Priority:    plugin.EditPriorityHigh,
			AfterCursor: false,
			Author:      "lsp",
			Delta:       delta,
			Rev:         view.Rev,
		}
		h.plugin.plugin.Edit(view, edit)
	case "didSave":
		view, ok := h.plugin.views[viewID]
		if !ok {
			return
		}
		lspClient, ok := h.plugin.lsp[view.Syntax]
		if !ok {
			return
		}
		lspClient.DidSave(view.Path)
	case "format":
		reply := ""
		defer conn.Reply(ctx, req.ID, reply)

		view, ok := h.plugin.views[viewID]
		if !ok {
			return
		}
		lspClient, ok := h.plugin.lsp[view.Syntax]
		if !ok {
			return
		}
		h.plugin.plugin.Mutex.Lock()
		defer h.plugin.plugin.Mutex.Unlock()

		log.Infoln("now format", view.Path)
		result, err := lspClient.Format(view.Path)
		if err != nil {
			log.Infoln(err)
			return
		}
		for _, edit := range result {
			log.Infoln("apply edit start")
			xiEdit := lspEditToXi(view, edit)
			for _, el := range xiEdit.Delta.Els {
				log.Infoln("apply edit", el)
			}
			h.plugin.plugin.Edit(view, xiEdit)
		}
	}
}

func lspEditToXi(view *plugin.View, edit *lsp.TextEdit) *plugin.Edit {
	start := edit.Range.Start
	end := edit.Range.End

	if start.Line == 0 && start.Character == 0 && view.GetOffset(end.Line, end.Character) == len(view.LineCache.Raw) {
		els := []*plugin.El{}
		oldRaw := view.LineCache.Raw
		newRaw := []byte(edit.NewText)
		oldLen := len(oldRaw)
		newLen := len(newRaw)
		i := 0
		for i = range oldRaw {
			if i >= newLen {
				break
			}
			if oldRaw[i] != newRaw[i] {
				break
			}
		}

		j := oldLen - 1
		newJ := newLen - (oldLen - j)
		for j = oldLen - 1; j > i; {
			if newJ <= 0 {
				break
			}
			if oldRaw[j] != newRaw[newJ] {
				break
			}
			j--
			newJ = newLen - (oldLen - j)
		}
		if newJ < i {
			i = newJ
		}

		el := &plugin.El{
			Copy: []int{0, i},
		}
		els = append(els, el)

		log.Info(i, j, newJ, oldLen, newLen)
		if newJ+1 > i {
			newText := string(newRaw[i : newJ+1])
			el = &plugin.El{
				Insert: newText,
			}
			els = append(els, el)
		}

		log.Infoln(i, j, newJ)
		el = &plugin.El{
			Copy: []int{j + 1, len(view.LineCache.Raw)},
		}
		els = append(els, el)
		delta := &plugin.Delta{
			BaseLen: len(view.LineCache.Raw),
			Els:     els,
		}
		log.Infoln(els)
		return &plugin.Edit{
			Priority:    plugin.EditPriorityHigh,
			AfterCursor: false,
			Author:      "lsp",
			Delta:       delta,
			Rev:         view.Rev,
		}
	}

	els := []*plugin.El{}
	el := &plugin.El{
		Copy: []int{0, view.GetOffset(start.Line, start.Character)},
	}
	els = append(els, el)
	el = &plugin.El{
		Insert: edit.NewText,
	}
	els = append(els, el)
	el = &plugin.El{
		Copy: []int{view.GetOffset(end.Line, end.Character), len(view.LineCache.Raw)},
	}
	els = append(els, el)
	delta := &plugin.Delta{
		BaseLen: len(view.LineCache.Raw),
		Els:     els,
	}

	return &plugin.Edit{
		Priority:    plugin.EditPriorityHigh,
		AfterCursor: false,
		Author:      "lsp",
		Delta:       delta,
		Rev:         view.Rev,
	}
}
