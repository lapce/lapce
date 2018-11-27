package plugin

import (
	"context"
	"encoding/json"
	"net"
	"runtime/debug"

	"github.com/crane-editor/crane/log"

	"github.com/crane-editor/crane/lsp"
	"github.com/crane-editor/crane/plugin"
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

func newServer(plugin *Plugin, addr string) (*Server, error) {
	log.Infoln("now listen")
	lis, err := net.Listen("tcp", addr)
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

func (s *Server) close() {
	if s.lis != nil {
		s.lis.Close()
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
		view, ok := h.plugin.Views[viewID]
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
		view, ok := h.plugin.Views[viewID]
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
		view, ok := h.plugin.Views[viewID]
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
		h.plugin.Mutex.Lock()
		defer h.plugin.Mutex.Unlock()

		var item *lsp.CompletionItem
		err = json.Unmarshal(paramsData, &item)
		if err != nil {
			return
		}
		view, ok := h.plugin.Views[viewID]
		if !ok {
			return
		}

		els := []*plugin.El{}
		el := &plugin.El{
			Copy: []int{0, h.plugin.getCompletionStart(view)},
		}
		els = append(els, el)
		el = &plugin.El{
			Insert: item.Label,
		}
		els = append(els, el)
		el = &plugin.El{
			Copy: []int{view.Cache.GetOffset(), len(view.Cache.GetContent())},
		}
		els = append(els, el)
		delta := &plugin.Delta{
			BaseLen: len(view.Cache.GetContent()),
			Els:     els,
		}
		edit := &plugin.Edit{
			Priority:    plugin.EditPriorityHigh,
			AfterCursor: false,
			Author:      "lsp",
			Delta:       delta,
			Rev:         view.Rev,
		}
		h.plugin.Edit(view, edit)
	case "format":
		reply := ""
		defer conn.Reply(ctx, req.ID, reply)

		view, ok := h.plugin.Views[viewID]
		if !ok {
			return
		}
		lspClient, ok := h.plugin.lsp[view.Syntax]
		if !ok {
			return
		}
		h.plugin.Mutex.Lock()
		defer h.plugin.Mutex.Unlock()

		log.Infoln("now format", view.Path)
		result, err := lspClient.Format(view.Path)
		if err != nil {
			log.Infoln(err)
			return
		}
		resultBytes, _ := json.Marshal(result)
		log.Infoln(string(resultBytes))

		els := []*plugin.El{}
		for _, edit := range result {
			elsItems := lspEditToXi(view, edit)
			if len(els) > 0 {
				lastEl := els[len(els)-1]
				lastEl.Copy[1] = elsItems[0].Copy[1]
				elsItems = elsItems[1:]
			}
			els = append(els, elsItems...)
		}
		if len(els) == 0 {
			return
		}
		delta := &plugin.Delta{
			BaseLen: len(view.Cache.GetContent()),
			Els:     els,
		}
		xiEdit := &plugin.Edit{
			Priority:    plugin.EditPriorityHigh,
			AfterCursor: false,
			Author:      "lsp",
			Delta:       delta,
			Rev:         view.Rev,
		}
		h.plugin.Edit(view, xiEdit)
	}
}

func lspEditToXi(view *plugin.View, edit *lsp.TextEdit) []*plugin.El {
	start := edit.Range.Start
	end := edit.Range.End
	content := view.Cache.GetContent()

	if start.Line == 0 && start.Character == 0 && view.Cache.PosToOffset(end.Line, end.Character) == len(content) {
		els := []*plugin.El{}
		oldRaw := content
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
			Copy: []int{j + 1, len(content)},
		}
		els = append(els, el)
		log.Infoln(els)
		return els
	}

	els := []*plugin.El{}
	el := &plugin.El{
		Copy: []int{0, view.Cache.PosToOffset(start.Line, start.Character)},
	}
	els = append(els, el)
	if edit.NewText != "" {
		el = &plugin.El{
			Insert: edit.NewText,
		}
		els = append(els, el)
	}

	offset := view.Cache.PosToOffset(end.Line, end.Character)
	if offset < len(content) {
		el = &plugin.El{
			Copy: []int{view.Cache.PosToOffset(end.Line, end.Character), len(content)},
		}
		els = append(els, el)
	} else if len(els) == 1 {
		els[0].Copy[1]--
	}

	return els
}
