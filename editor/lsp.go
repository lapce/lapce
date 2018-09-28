package editor

import (
	"context"
	"encoding/json"
	"net"
	"runtime/debug"

	"github.com/crane-editor/crane/log"

	"github.com/crane-editor/crane/lsp"
	plugin "github.com/crane-editor/crane/lsp-plugin"
	"github.com/sourcegraph/jsonrpc2"
)

// LspClient is
type LspClient struct {
	editor *Editor
	conn   *jsonrpc2.Conn
}

func newLspClient(editor *Editor, conn net.Conn) *LspClient {
	l := &LspClient{
		editor: editor,
	}
	l.conn = jsonrpc2.NewConn(context.Background(), plugin.NewConnStream(conn), l)
	return l
}

// Handle is
func (l *LspClient) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
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
	// log.Infoln("now handle", req.ID, req.Method, string(paramsData))
	switch req.Method {
	case "completion":
		var items []*lsp.CompletionItem
		err = json.Unmarshal(paramsData, &items)
		if err != nil {
			log.Infoln("json error", err)
			return
		}
		l.editor.popup.updateItems(items)
	case "completion_pos":
		var pos *lsp.Position
		err = json.Unmarshal(paramsData, &pos)
		if err != nil {
			log.Infoln("json error", err)
			return
		}
		l.editor.popup.updatePos(pos)
	case "definition":
		var location *lsp.Location
		err = json.Unmarshal(paramsData, &location)
		if err != nil {
			log.Infoln("json error", err)
			return
		}
		l.editor.updates <- location
		l.editor.signal.UpdateSignal()
	case "diagnostics":
		var params *lsp.PublishDiagnosticsParams
		err = json.Unmarshal(paramsData, &params)
		if err != nil {
			log.Infoln("json error", err)
			return
		}
		l.editor.updates <- params
		l.editor.signal.UpdateSignal()
	}
}

func (l *LspClient) definition(buffer *Buffer, row int, col int) {
	pos := lsp.Position{
		Line:      row,
		Character: col,
	}
	params := &lsp.TextDocumentPositionParams{
		TextDocument: lsp.TextDocumentIdentifier{
			URI: "file://" + buffer.path,
		},
		Position: pos,
	}
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	l.conn.Notify(context.Background(), "definition", params, jsonrpc2.Meta(meta))
}

func (l *LspClient) hover(buffer *Buffer, row int, col int) {
	pos := lsp.Position{
		Line:      row,
		Character: col,
	}
	params := &lsp.TextDocumentPositionParams{
		TextDocument: lsp.TextDocumentIdentifier{
			URI: "file://" + buffer.path,
		},
		Position: pos,
	}
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	l.conn.Notify(context.Background(), "hover", params, jsonrpc2.Meta(meta))
}

func (l *LspClient) format(buffer *Buffer) {
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	var result interface{}
	l.conn.Call(context.Background(), "format", nil, &result, jsonrpc2.Meta(meta))
}

func (l *LspClient) didSave(buffer *Buffer) {
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	l.conn.Notify(context.Background(), "didSave", nil, jsonrpc2.Meta(meta))
}

func (l *LspClient) completion(buffer *Buffer, row int, col int) {
	params := &lsp.TextDocumentPositionParams{
		TextDocument: lsp.TextDocumentIdentifier{
			URI: "file://" + buffer.path,
		},
		Position: lsp.Position{
			Line:      row,
			Character: col,
		},
	}
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	var result *lsp.CompletionResp
	l.conn.Call(context.Background(), "completion", params, &result, jsonrpc2.Meta(meta))
	for _, item := range result.Items {
		log.Infoln(item.InsertText)
	}
	log.Infoln(row, col, buffer.xiView.ID, buffer.path)
}

func (l *LspClient) selectCompletionItem(buffer *Buffer, item *lsp.CompletionItem) {
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	l.conn.Notify(context.Background(), "completion_select", item, jsonrpc2.Meta(meta))
}

func (l *LspClient) resetCompletion(buffer *Buffer) {
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	params := map[string]string{}
	l.conn.Notify(context.Background(), "completion_reset", params, jsonrpc2.Meta(meta))
}
