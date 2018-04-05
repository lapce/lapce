package plugin

import (
	"context"
	"encoding/json"
	"log"
	"net"
	"runtime/debug"

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
	log.Println("now listen")
	lis, err := net.Listen("tcp", "127.0.0.1:50051")
	if err != nil {
		log.Println("now listen", err)
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
	log.Println("now serve", conn.RemoteAddr().String())
	s.plugin.conns[conn.RemoteAddr().String()] = jsonrpc2.NewConn(context.Background(), NewConnStream(conn), &handler{plugin: s.plugin})
}

func (h *handler) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	defer func() {
		if r := recover(); r != nil {
			log.Println("handle error", r, string(debug.Stack()))
		}
	}()
	paramsData, err := req.Params.MarshalJSON()
	if err != nil {
		log.Println(err)
		return
	}
	log.Println("now handle", req.ID, req.Method, string(paramsData))
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
	log.Println("view id", viewID)
	switch req.Method {
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
		log.Println("get resp", resp)
		conn.Reply(ctx, req.ID, resp)
		log.Println("resp replied")
	case "completion_reset":
		h.plugin.completionItems = []*lsp.CompletionItem{}
	case "completion_select":
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
	}
}
