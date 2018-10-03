package lsp

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/crane-editor/crane/log"

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

type handleNotificationFunc func(notification interface{})

type handler struct {
	client *Client
}

// Client is a lsp client
type Client struct {
	Conn               *jsonrpc2.Conn
	handleNotification handleNotificationFunc
	ServerCapabilities *Capabilities
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
	Range       *Range `json:"range,omitempty"`
	RangeLength *int   `json:"rangeLength,omitempty"`
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

// DocumentFormattingParams is
type DocumentFormattingParams struct {
	TextDocument *TextDocumentIdentifier `json:"textDocument"`
}

// CompletionResp isj
type CompletionResp struct {
	IsIncomplete bool              `json:"isIncomplete"`
	Items        []*CompletionItem `json:"items"`
}

// Capabilities is
type Capabilities struct {
	CompletionProvider struct {
		TriggerCharacters []string `json:"triggerCharacters"`
	} `json:"completionProvider"`
	DefinitionProvider         bool `json:"definitionProvider"`
	DocumentFormattingProvider bool `json:"documentFormattingProvider"`
	DocumentSymbolProvider     bool `json:"documentSymbolProvider"`
	HoverProvider              bool `json:"hoverProvider"`
	ImplementationProvider     bool `json:"implementationProvider"`
	ReferencesProvider         bool `json:"referencesProvider"`
	SignatureHelpProvider      struct {
		TriggerCharacters []string `json:"triggerCharacters"`
	} `json:"signatureHelpProvider"`
	TextDocumentSync             int  `json:"textDocumentSync"`
	TypeDefinitionProvider       bool `json:"typeDefinitionProvider"`
	WorkspaceSymbolProvider      bool `json:"workspaceSymbolProvider"`
	XdefinitionProvider          bool `json:"xdefinitionProvider"`
	XworkspaceReferencesProvider bool `json:"xworkspaceReferencesProvider"`
	XworkspaceSymbolByProperties bool `json:"xworkspaceSymbolByProperties"`
}

// InitializeResult is
type InitializeResult struct {
	Capabilities *Capabilities `json:"capabilities"`
}

// TextEdit is
type TextEdit struct {
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
}

// CompletionItem is
type CompletionItem struct {
	InsertText       string   `json:"insertText"`
	InsertTextFormat int      `json:"insertTextFormat"`
	Kind             int      `json:"kind"`
	Label            string   `json:"label"`
	TextEdit         TextEdit `json:"textEdit"`
	Detail           string   `json:"detail,omitempty"`

	Score   int   `json:"-"`
	Matches []int `json:"matches"`
}

// Diagnostics is
type Diagnostics struct {
	Range   *Range `json:"range"`
	Source  string `json:"source"`
	Message string `json:"message"`
}

// PublishDiagnosticsParams is
type PublishDiagnosticsParams struct {
	URI         string         `json:"uri"`
	Diagnostics []*Diagnostics `json:"diagnostics"`
}

// Location is
type Location struct {
	Range *Range `json:"range"`
	URI   string `json:"uri"`
}

// Handle implements jsonrpc2.Handler
func (h *handler) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	paramsData, err := req.Params.MarshalJSON()
	if err != nil {
		log.Infoln(err)
		return
	}
	switch req.Method {
	case "textDocument/publishDiagnostics":
		var params *PublishDiagnosticsParams
		err = json.Unmarshal(paramsData, &params)
		if err != nil {
			log.Infoln(err)
			return
		}
		h.client.handleNotification(params)
	}
}

// NewClient is
func NewClient(syntax string, handleNotificationFunc handleNotificationFunc) (*Client, error) {
	cmd := ""
	args := []string{}
	switch syntax {
	case "go":
		cmd = "go-langserver"
		args = []string{"-gocodecompletion", "-lint-tool", "golint"}
	case "py":
		cmd = "pyls"
	case "c":
		cmd = "cquery"
	case "css":
		cmd = "css-languageserver"
		args = []string{"--stdio"}
	case "scss":
		cmd = "css-languageserver"
		args = []string{"--stdio"}
	case "html":
		cmd = "html-languageserver"
		args = []string{"--stdio"}
	default:
		return nil, errors.New("syntax " + syntax + " lsp not supported")
	}
	log.Infoln("new lsp client", cmd, args)
	stream, err := NewStdinoutStream(cmd, args...)
	if err != nil {
		return nil, err
	}

	c := &Client{}
	conn := jsonrpc2.NewConn(context.Background(), stream, &handler{client: c})
	c.Conn = conn
	c.handleNotification = handleNotificationFunc
	return c, nil
}

// Initialize the lsp
func (c *Client) Initialize(rootPath string) error {
	params := map[string]interface{}{}
	params["rootPath"] = rootPath
	params["capabilities"] = map[string]interface{}{
		"workspace": map[string]interface{}{},
	}
	var result *InitializeResult
	err := c.Conn.Call(context.Background(), "initialize", &params, &result)
	if err != nil {
		return err
	}
	c.ServerCapabilities = result.Capabilities
	log.Infoln("initialize", err, result, rootPath)
	return nil
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

// DidSave is
func (c *Client) DidSave(path string) error {
	textDocument := map[string]string{}
	textDocument["uri"] = "file://" + path
	params := map[string]interface{}{}
	params["textDocument"] = textDocument
	err := c.Conn.Notify(context.Background(), "textDocument/didSave", &params)
	log.Infoln("lsp send didSave")
	return err
}

// DidChange is
func (c *Client) DidChange(didChangeParams *DidChangeParams) error {
	var result interface{}
	err := c.Conn.Call(context.Background(), "textDocument/didChange", didChangeParams, &result)
	log.Infoln("did change error", err, result)
	return err
}

// Format is
func (c *Client) Format(path string) ([]*TextEdit, error) {
	var result []*TextEdit
	params := &DocumentFormattingParams{
		TextDocument: &TextDocumentIdentifier{
			URI: "file://" + path,
		},
	}
	err := c.Conn.Call(context.Background(), "textDocument/formatting", params, &result)
	return result, err
}

// Definition is
func (c *Client) Definition(params *TextDocumentPositionParams) ([]*Location, error) {
	var result []*Location
	log.Infoln("get definition")
	err := c.Conn.Call(context.Background(), "textDocument/definition", &params, &result)
	buf, _ := json.Marshal(result)
	log.Infoln(err, string(buf))
	return result, err
}

// Hover is
func (c *Client) Hover(params *TextDocumentPositionParams) {
	var result interface{}
	log.Infoln("get hover")
	err := c.Conn.Call(context.Background(), "textDocument/hover", &params, &result)
	buf, _ := json.Marshal(result)
	log.Infoln(err, string(buf))
}

// Signature is
func (c *Client) Signature(params *TextDocumentPositionParams) {
	var result interface{}
	log.Infoln("get signature")
	err := c.Conn.Call(context.Background(), "textDocument/signatureHelp", &params, &result)
	log.Infoln(err, result)
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
