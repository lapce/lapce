package editor

import (
	"context"
	"encoding/json"
	"log"
	"net"
	"runtime/debug"

	"github.com/dzhou121/crane/lsp"
	plugin "github.com/dzhou121/crane/lsp-plugin"
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
			log.Println("handle error", r, string(debug.Stack()))
		}
	}()
	paramsData, err := req.Params.MarshalJSON()
	if err != nil {
		log.Println(err)
		return
	}
	// log.Println("now handle", req.ID, req.Method, string(paramsData))
	switch req.Method {
	case "completion":
		var items []*lsp.CompletionItem
		err = json.Unmarshal(paramsData, &items)
		if err != nil {
			log.Println("json error", err)
			return
		}
		l.editor.popup.updateItems(items)
	case "completion_pos":
		var pos *lsp.Position
		err = json.Unmarshal(paramsData, &pos)
		if err != nil {
			log.Println("json error", err)
			return
		}
		l.editor.popup.updatePos(pos)
	}
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
		log.Println(item.InsertText)
	}
	log.Println(row, col, buffer.xiView.ID, buffer.path)
}

func (l *LspClient) selectCompletionItem(buffer *Buffer, item *lsp.CompletionItem) {
	meta := map[string]string{
		"view_id": buffer.xiView.ID,
	}
	l.conn.Notify(context.Background(), "completion_select", item, jsonrpc2.Meta(meta))
}
