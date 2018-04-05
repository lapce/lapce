package lsp

import (
	"context"
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
	URI     string
	Version *int
}

// TextDocumentIdentifier is
type TextDocumentIdentifier struct {
	URI     string
	Version *int
}

// Position is
type Position struct {
	Line      int
	Character int
}

// Range is
type Range struct {
	Start *Position
	End   *Position
}

// ContentChange is
type ContentChange struct {
	Range       *Range
	RangeLength *int `json:"rangeLength"`
	Text        string
}

// DidChangeParams is
type DidChangeParams struct {
	TextDocument   VersionedTextDocumentIdentifier `json:"textDocument"`
	ContentChanges []*ContentChange                `json:"contentChanges"`
}

// TextDocumentPositionParams is
type TextDocumentPositionParams struct {
	TextDocument TextDocumentIdentifier `json:"textDocument"`
	Position     Position
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

// Handle implements jsonrpc2.Handler
func (h *handler) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	log.Println("got notification", req)
}

// NewClient is
func NewClient() (*Client, error) {
	stream, err := NewStdinoutStream("go-langserver", "-gocodecompletion")
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
	log.Println("initialize", err, result)
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
	log.Println("did open", err, result)
	return err
}

// DidChange is
func (c *Client) DidChange(didChangeParams *DidChangeParams) error {
	var result interface{}
	err := c.Conn.Call(context.Background(), "textDocument/didChange", didChangeParams, &result)
	log.Println("did change", err, result)
	return err
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
