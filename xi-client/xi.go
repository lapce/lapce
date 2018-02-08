package xi

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"os/exec"

	"github.com/sourcegraph/jsonrpc2"
)

type handleUpdateFunc func(notification *UpdateNotification)
type handleScrolltoFunc func(scrollto *ScrollTo)

// Xi represents an instance of xi-core
type Xi struct {
	Conn           *jsonrpc2.Conn
	HandleUpdate   handleUpdateFunc
	HandleScrollto handleScrolltoFunc
}

// View is a Xi view
type View struct {
	xi *Xi
	ID string
}

// NewViewParams is
type NewViewParams struct {
	Path string `json:"file_path,omitempty"`
}

// New creates a Xi client
func New() (*Xi, error) {
	cmd := exec.Command("xi-core")
	inw, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}

	outr, err := cmd.StdoutPipe()
	if err != nil {
		inw.Close()
		return nil, err
	}

	err = cmd.Start()
	if err != nil {
		return nil, err
	}

	stream := &stdinoutStream{
		in:     inw,
		out:    outr,
		reader: bufio.NewReader(outr),
	}
	xi := &Xi{}
	conn := jsonrpc2.NewConn(context.Background(), stream, &handler{xi: xi})
	xi.Conn = conn
	return xi, nil
}

// NewView creats a new view
func (x *Xi) NewView(path string) (*View, error) {
	viewID := ""
	params := &NewViewParams{
		Path: path,
	}
	err := x.Conn.Call(context.Background(), "new_view", &params, &viewID)
	if err != nil {
		return nil, err
	}
	return &View{
		xi: x,
		ID: viewID,
	}, nil
}

type stdinoutStream struct {
	in     io.WriteCloser
	out    io.ReadCloser
	reader *bufio.Reader
}

// WriteObject implements ObjectStream.
func (s *stdinoutStream) WriteObject(obj interface{}) error {
	data, err := json.Marshal(obj)
	if err != nil {
		return err
	}
	data = append(data, '\n')
	_, err = s.in.Write(data)
	return err
}

// ReadObject implements ObjectStream.
func (s *stdinoutStream) ReadObject(v interface{}) error {
	line, err := s.reader.ReadSlice('\n')
	if err != nil {
		return err
	}
	err = json.Unmarshal(line, v)
	return err
}

// Close implements ObjectStream.
func (s *stdinoutStream) Close() error {
	err := s.in.Close()
	if err != nil {
		return err
	}
	return s.out.Close()
}

type handler struct {
	xi *Xi
}

// Handle implements jsonrpc2.Handler
func (h *handler) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	params, err := req.Params.MarshalJSON()
	if err != nil {
		return
	}
	// fmt.Println("-------------------------")
	// fmt.Println(req.Method)
	// fmt.Println(string(params))
	switch req.Method {
	case "update":
		var notification UpdateNotification
		err := json.Unmarshal(params, &notification)
		if err != nil {
			return
		}
		if h.xi.HandleUpdate != nil {
			h.xi.HandleUpdate(&notification)
		}
	case "scroll_to":
		var scrollTo ScrollTo
		err := json.Unmarshal(params, &scrollTo)
		if err != nil {
			return
		}
		if h.xi.HandleScrollto != nil {
			h.xi.HandleScrollto(&scrollTo)
		}
	default:
	}
}

// ScrollTo is
type ScrollTo struct {
	Col    int    `json:"col"`
	Line   int    `json:"line"`
	ViewID string `json:"view_id"`
}

// EditNotification is
type EditNotification struct {
	method string
	cmd    *EditCommand `json:"params"`
}

// EditCommand is
type EditCommand struct {
	ViewID string      `json:"view_id"`
	Method string      `json:"method"`
	Params interface{} `json:"params,omitempty"`
}

// Insert chars at the current cursor location
func (v *View) Insert(chars string) {
	params := map[string]string{}
	params["chars"] = chars

	cmd := &EditCommand{
		Method: "insert",
		ViewID: v.ID,
		Params: params,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Scroll sets
func (v *View) Scroll() {
	cmd := &EditCommand{
		Method: "scroll",
		ViewID: v.ID,
		Params: []int{0, 20},
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// RequestLines sets
func (v *View) RequestLines() {
	cmd := &EditCommand{
		Method: "request_lines",
		ViewID: v.ID,
		Params: []int{0, 20},
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveUp is
func (v *View) MoveUp() {
	cmd := &EditCommand{
		Method: "move_up",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveDown is
func (v *View) MoveDown() {
	cmd := &EditCommand{
		Method: "move_down",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveToEndOfDocument is
func (v *View) MoveToEndOfDocument() {
	cmd := &EditCommand{
		Method: "move_to_end_of_document",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// ScrollPageUp is
func (v *View) ScrollPageUp() {
	cmd := &EditCommand{
		Method: "scroll_page_up",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// ScrollPageDown is
func (v *View) ScrollPageDown() {
	cmd := &EditCommand{
		Method: "scroll_page_down",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveWordLeft moves to word left
func (v *View) MoveWordLeft() {
	cmd := &EditCommand{
		Method: "move_word_left",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// InsertNewline inserts a new line
func (v *View) InsertNewline() {
	cmd := &EditCommand{
		Method: "insert_new_line",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Line is
type Line struct {
	Cursor []int64       `json:"cursor"`
	Styles []interface{} `json:"styles"`
	Text   string        `json:"text"`
}

// UpdateOperation is
type UpdateOperation struct {
	n         int64   `json:"n"`
	Operation string  `json:"op"`
	Lines     []*Line `json:"lines"`
}

// UpdateNotification is
type UpdateNotification struct {
	Update struct {
		Ops []struct {
			Lines []struct {
				Cursor []int         `json:"cursor"`
				Styles []interface{} `json:"styles"`
				Text   string        `json:"text"`
			} `json:"lines"`
			N  int    `json:"n"`
			Op string `json:"op"`
		} `json:"ops"`
		Pristine bool `json:"pristine"`
	} `json:"update"`
	ViewID string `json:"view_id"`
}
