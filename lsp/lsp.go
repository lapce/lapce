package lsp

import (
	"context"
	"encoding/json"
	"log"

	"github.com/sourcegraph/jsonrpc2"
)

//
const (
	Text          = 1
	Method        = 2
	Function      = 3
	Constructor   = 4
	Field         = 5
	Variable      = 6
	Class         = 7
	Interface     = 8
	Module        = 9
	Property      = 10
	Unit          = 11
	Value         = 12
	Enum          = 13
	Keyword       = 14
	Snippet       = 15
	Color         = 16
	File          = 17
	Reference     = 18
	Folder        = 19
	EnumMember    = 20
	Constant      = 21
	Struct        = 22
	Event         = 23
	Operator      = 24
	TypeParameter = 25
)

type handler struct {
	client *Client
}

// Client is a lsp client
type Client struct {
	Conn *jsonrpc2.Conn
}

// VersionedTextDocumentIdentifier is
type VersionedTextDocumentIdentifier struct {
	URI     string `json:"uri"`
	Version *int   `json:"version,omitempty"`
}

// TextDocumentIdentifier is
type TextDocumentIdentifier struct {
	URI string `json:"uri"`
}

// Position is
type Position struct {
	Line      int `json:"line"`
	Character int `json:"character"`
}

// Range is
type Range struct {
	Start *Position `json:"start"`
	End   *Position `json:"end"`
}

// ContentChange is
type ContentChange struct {
	Range       *Range `json:"range"`
	RangeLength *int   `json:"rangeLength"`
	Text        string `json:"text"`
}

// DidChangeParams is
type DidChangeParams struct {
	TextDocument   VersionedTextDocumentIdentifier `json:"textDocument"`
	ContentChanges []*ContentChange                `json:"contentChanges"`
}

// TextDocumentPositionParams is
type TextDocumentPositionParams struct {
	TextDocument TextDocumentIdentifier `json:"textDocument"`
	Position     Position               `json:"position"`
}

// CompletionResp isj
type CompletionResp struct {
	IsIncomplete bool              `json:"isIncomplete"`
	Items        []*CompletionItem `json:"items"`
}

// CompletionItem is
type CompletionItem struct {
	InsertText       string `json:"insertText"`
	InsertTextFormat int    `json:"insertTextFormat"`
	Kind             int    `json:"kind"`
	Label            string `json:"label"`
	TextEdit         struct {
		NewText string `json:"newText"`
		Range   struct {
			End struct {
				Character int `json:"character"`
				Line      int `json:"line"`
			} `json:"end"`
			Start struct {
				Character int `json:"character"`
				Line      int `json:"line"`
			} `json:"start"`
		} `json:"range"`
	} `json:"textEdit"`
	Detail string `json:"detail,omitempty"`

	Score   int   `json:"-"`
	Matches []int `json:"matches"`
}

// Location is
type Location struct {
	Range struct {
		End struct {
			Character int `json:"character"`
			Line      int `json:"line"`
		} `json:"end"`
		Start struct {
			Character int `json:"character"`
			Line      int `json:"line"`
		} `json:"start"`
	} `json:"range"`
	URI string `json:"uri"`
}

// Handle implements jsonrpc2.Handler
func (h *handler) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	log.Println("got notification", req)
}

// NewClient is
func NewClient(syntax string) (*Client, error) {
	cmd := ""
	args := []string{}
	switch syntax {
	case "go":
		cmd = "go-langserver"
		args = []string{"-gocodecompletion"}
	case "python":
		cmd = "pyls"
	}
	stream, err := NewStdinoutStream(cmd, args...)
	if err != nil {
		return nil, err
	}

	c := &Client{}
	conn := jsonrpc2.NewConn(context.Background(), stream, &handler{client: c})
	c.Conn = conn
	return c, nil
}

// Initialize the lsp
func (c *Client) Initialize(rootPath string) error {
	params := map[string]string{}
	params["rootPath"] = rootPath
	var result interface{}
	err := c.Conn.Call(context.Background(), "initialize", &params, &result)
	log.Println("initialize", err, result, rootPath)
	return err
}

// DidOpen is
func (c *Client) DidOpen(path string, content string) error {
	textDocument := map[string]string{}
	textDocument["uri"] = "file://" + path
	textDocument["text"] = content
	params := map[string]interface{}{}
	params["textDocument"] = textDocument
	var result interface{}
	err := c.Conn.Call(context.Background(), "textDocument/didOpen", &params, &result)
	return err
}

// DidChange is
func (c *Client) DidChange(didChangeParams *DidChangeParams) error {
	var result interface{}
	err := c.Conn.Call(context.Background(), "textDocument/didChange", didChangeParams, &result)
	log.Println("did change", err, result)
	return err
}

// Definition is
func (c *Client) Definition(params *TextDocumentPositionParams) ([]*Location, error) {
	var result []*Location
	log.Println("get definition")
	err := c.Conn.Call(context.Background(), "textDocument/definition", &params, &result)
	buf, _ := json.Marshal(result)
	log.Println(err, string(buf))
	return result, err
}

// Hover is
func (c *Client) Hover(params *TextDocumentPositionParams) {
	var result interface{}
	log.Println("get hover")
	err := c.Conn.Call(context.Background(), "textDocument/hover", &params, &result)
	buf, _ := json.Marshal(result)
	log.Println(err, string(buf))
}

// Signature is
func (c *Client) Signature(params *TextDocumentPositionParams) {
	var result interface{}
	log.Println("get signature")
	err := c.Conn.Call(context.Background(), "textDocument/signatureHelp", &params, &result)
	log.Println(err, result)
}

// Completion is
func (c *Client) Completion(params *TextDocumentPositionParams) (*CompletionResp, error) {
	var completionResp CompletionResp
	err := c.Conn.Call(context.Background(), "textDocument/completion", &params, &completionResp)
	return &completionResp, err
}

// CompletionResolve is
func (c *Client) CompletionResolve(item *CompletionItem) error {
	return c.Conn.Call(context.Background(), "completionItem/resolve", &item, &item)
}
