package xi

import (
	"context"
	"encoding/json"
	"io"
	"os/exec"

	"github.com/crane-editor/crane/log"
	"github.com/sourcegraph/jsonrpc2"
)

//
const (
	PointSelect = "point_select"
	RangeSelect = "range_select"
)

type handleNotificationFunc func(notification interface{})

// Xi represents an instance of xi-core
type Xi struct {
	Conn               *jsonrpc2.Conn
	handleNotification handleNotificationFunc
}

// View is a Xi view
type View struct {
	xi   *Xi
	ID   string
	Path string
}

// NewViewParams is
type NewViewParams struct {
	Path string `json:"file_path,omitempty"`
}

// New creates a Xi client
func New(handleNotification handleNotificationFunc) (*Xi, error) {
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

	stderr, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	go func() {
		buf := make([]byte, 1000)
		for {
			n, err := stderr.Read(buf)
			if err != nil {
				return
			}
			log.Infoln("xi-core stderr:", string(buf[:n]))
		}
	}()

	err = cmd.Start()
	if err != nil {
		return nil, err
	}

	stream := &StdinoutStream{
		in:      inw,
		out:     outr,
		decoder: json.NewDecoder(outr),
		encoder: json.NewEncoder(inw),
	}
	xi := &Xi{
		handleNotification: handleNotification,
	}
	conn := jsonrpc2.NewConn(context.Background(), stream, &handler{xi: xi})
	xi.Conn = conn
	return xi, nil
}

// ClientStart is
func (x *Xi) ClientStart(configDir string) {
	params := map[string]string{}
	params["client_extras_dir"] = configDir
	params["config_dir"] = configDir
	x.Conn.Notify(context.Background(), "client_started", &params)
}

// SetTheme sets theme
func (x *Xi) SetTheme(themeName string) {
	params := map[string]string{}
	params["theme_name"] = themeName
	x.Conn.Notify(context.Background(), "set_theme", &params)
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
		xi:   x,
		ID:   viewID,
		Path: path,
	}, nil
}

// StdinoutStream is
type StdinoutStream struct {
	in      io.WriteCloser
	out     io.ReadCloser
	decoder *json.Decoder
	encoder *json.Encoder
}

// WriteObject implements ObjectStream.
func (s *StdinoutStream) WriteObject(obj interface{}) error {
	data, err := json.Marshal(obj)
	if err != nil {
		return err
	}
	data = append(data, '\n')
	_, err = s.in.Write(data)
	return err
}

// ReadObject implements ObjectStream.
func (s *StdinoutStream) ReadObject(v interface{}) error {
	err := s.decoder.Decode(v)
	if err != nil {
		log.Infoln("read object err", err)
	}
	return err
}

// Close implements ObjectStream.
func (s *StdinoutStream) Close() error {
	return nil
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
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&notification)
		}
	case "scroll_to":
		var scrollTo ScrollTo
		err := json.Unmarshal(params, &scrollTo)
		if err != nil {
			return
		}
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&scrollTo)
		}
	case "def_style":
		var style Style
		err := json.Unmarshal(params, &style)
		if err != nil {
			return
		}
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&style)
		}
	case "theme_changed":
		var theme Theme
		err := json.Unmarshal(params, &theme)
		if err != nil {
			return
		}
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&theme)
		}
	case "config_changed":
		var configChanged ConfigChanged
		err := json.Unmarshal(params, &configChanged)
		if err != nil {
			return
		}
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&configChanged)
		}
	case "available_themes":
		var themes Themes
		err := json.Unmarshal(params, &themes)
		if err != nil {
			return
		}
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&themes)
		}
	case "measure_width":
		var widthParams []*MeasureWidthParams
		err := json.Unmarshal(params, &widthParams)
		if err != nil {
			return
		}
		if h.xi.handleNotification != nil {
			h.xi.handleNotification(&MeasureWidthRequest{
				ID:     req.ID,
				Params: widthParams,
			})
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

// Style is
type Style struct {
	FgColor int `json:"fg_color"`
	ID      int `json:"id"`
}

// Themes is
type Themes struct {
	Themes []string `json:"themes"`
}

// Color is
type Color struct {
	R int `json:"r"`
	G int `json:"g"`
	B int `json:"b"`
	A int `json:"a"`
}

// Theme is
type Theme struct {
	Name  string `json:"name"`
	Theme struct {
		Accent      interface{} `json:"accent"`
		ActiveGuide struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"active_guide"`
		Background struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"background"`
		BracketContentsForeground interface{} `json:"bracket_contents_foreground"`
		BracketContentsOptions    interface{} `json:"bracket_contents_options"`
		BracketsBackground        interface{} `json:"brackets_background"`
		BracketsForeground        interface{} `json:"brackets_foreground"`
		BracketsOptions           interface{} `json:"brackets_options"`
		Caret                     struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"caret"`
		FindHighlight           interface{} `json:"find_highlight"`
		FindHighlightForeground interface{} `json:"find_highlight_foreground"`
		Foreground              struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"foreground"`
		Guide struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"guide"`
		Gutter struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"gutter"`
		GutterForeground            *Color      `json:"gutter_foreground"`
		Highlight                   interface{} `json:"highlight"`
		HighlightForeground         interface{} `json:"highlight_foreground"`
		InactiveSelection           interface{} `json:"inactive_selection"`
		InactiveSelectionForeground interface{} `json:"inactive_selection_foreground"`
		LineHighlight               struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"line_highlight"`
		MinimapBorder       interface{} `json:"minimap_border"`
		Misspelling         interface{} `json:"misspelling"`
		PhantomCSS          interface{} `json:"phantom_css"`
		PopupCSS            interface{} `json:"popup_css"`
		Selection           *Color      `json:"selection"`
		SelectionBackground interface{} `json:"selection_background"`
		SelectionBorder     interface{} `json:"selection_border"`
		SelectionForeground interface{} `json:"selection_foreground"`
		Shadow              interface{} `json:"shadow"`
		StackGuide          struct {
			A int `json:"a"`
			B int `json:"b"`
			G int `json:"g"`
			R int `json:"r"`
		} `json:"stack_guide"`
		TagsForeground interface{} `json:"tags_foreground"`
		TagsOptions    interface{} `json:"tags_options"`
	} `json:"theme"`
}

// ConfigChanged is
type ConfigChanged struct {
	Changes Config `json:"changes"`
	ViewID  string `json:"view_id"`
}

// Config is
type Config struct {
	AutoIndent            bool          `json:"auto_indent"`
	FontFace              string        `json:"font_face"`
	FontSize              int           `json:"font_size"`
	LineEnding            string        `json:"line_ending"`
	PluginSearchPath      []interface{} `json:"plugin_search_path"`
	ScrollPastEnd         bool          `json:"scroll_past_end"`
	TabSize               int           `json:"tab_size"`
	TranslateTabsToSpaces bool          `json:"translate_tabs_to_spaces"`
	UseTabStops           bool          `json:"use_tab_stops"`
	WrapWidth             int           `json:"wrap_width"`
}

// Notification is
type Notification struct {
	Method string      `json:"method"`
	Params interface{} `json:"params"`
}

// EditNotification is
type EditNotification struct {
	method string
	cmd    *EditCommand `json:"params"`
}

// PlaceholderRPC is
type PlaceholderRPC struct {
	Method  string      `json:"method"`
	Params  interface{} `json:"params,omitempty"`
	RPCType string      `json:"rpc_type"`
}

// PluginNotification is
type PluginNotification struct {
	Command  string          `json:"command"`
	ViewID   string          `json:"view_id"`
	Receiver string          `json:"receiver"`
	RPC      *PlaceholderRPC `json:"rpc"`
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

// GotoLine sets
func (v *View) GotoLine(line int) {
	params := map[string]int{}
	params["line"] = line

	cmd := &EditCommand{
		Method: "goto_line",
		ViewID: v.ID,
		Params: params,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Scroll sets
func (v *View) Scroll(start, end int) {
	cmd := &EditCommand{
		Method: "scroll",
		ViewID: v.ID,
		Params: []int{start, end},
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Click sets
func (v *View) Click(row, col int) {
	v.Gesture(row, col, PointSelect)
}

// Gesture sets
func (v *View) Gesture(row, col int, ty string) {
	params := map[string]interface{}{
		"line": row,
		"col":  col,
		"ty":   ty,
	}
	cmd := &EditCommand{
		Method: "gesture",
		ViewID: v.ID,
		Params: params,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Resize sets
func (v *View) Resize(width, height int) {
	params := map[string]interface{}{
		"width":  width,
		"height": height,
	}
	cmd := &EditCommand{
		Method: "resize",
		ViewID: v.ID,
		Params: params,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Drag sets
func (v *View) Drag(row, col int) {
	cmd := &EditCommand{
		Method: "drag",
		ViewID: v.ID,
		Params: []int{row, col, 0},
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// AddSelectionAbove is
func (v *View) AddSelectionAbove() {
	cmd := &EditCommand{
		Method: "add_selection_above",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// AddSelectionBelow is
func (v *View) AddSelectionBelow() {
	cmd := &EditCommand{
		Method: "add_selection_below",
		ViewID: v.ID,
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

// MoveLeft is
func (v *View) MoveLeft() {
	cmd := &EditCommand{
		Method: "move_left",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveRight is
func (v *View) MoveRight() {
	cmd := &EditCommand{
		Method: "move_right",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveRightAndModifySelection is
func (v *View) MoveRightAndModifySelection() {
	cmd := &EditCommand{
		Method: "move_right_and_modify_selection",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveToLeftEndOfLine is
func (v *View) MoveToLeftEndOfLine() {
	cmd := &EditCommand{
		Method: "move_to_left_end_of_line",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// MoveToRightEndOfLine is
func (v *View) MoveToRightEndOfLine() {
	cmd := &EditCommand{
		Method: "move_to_right_end_of_line",
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

// MoveToBeginningOfDocument is
func (v *View) MoveToBeginningOfDocument() {
	cmd := &EditCommand{
		Method: "move_to_beginning_of_document",
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

// MoveWordRight is
func (v *View) MoveWordRight() {
	cmd := &EditCommand{
		Method: "move_word_right",
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
		Method: "insert_newline",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// InsertTab inserts a new tab
func (v *View) InsertTab() {
	cmd := &EditCommand{
		Method: "insert_tab",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// DeleteBackward deletes backwards
func (v *View) DeleteBackward() {
	cmd := &EditCommand{
		Method: "delete_backward",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// DeleteForward deletes forwards
func (v *View) DeleteForward() {
	cmd := &EditCommand{
		Method: "delete_forward",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// DeleteWordBackward deletes backwards
func (v *View) DeleteWordBackward() {
	cmd := &EditCommand{
		Method: "delete_word_backward",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// DeleteToBeginningOfLine deletes
func (v *View) DeleteToBeginningOfLine() {
	cmd := &EditCommand{
		Method: "delete_to_beginning_of_line",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Undo is
func (v *View) Undo() {
	cmd := &EditCommand{
		Method: "undo",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Save is
func (v *View) Save() {
	params := map[string]string{}
	params["view_id"] = v.ID
	params["file_path"] = v.Path
	v.xi.Conn.Notify(context.Background(), "save", &params)
}

// Redo is
func (v *View) Redo() {
	cmd := &EditCommand{
		Method: "redo",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// CancelOperation deletes forwards
func (v *View) CancelOperation() {
	cmd := &EditCommand{
		Method: "cancel_operation",
		ViewID: v.ID,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// Find finds
func (v *View) Find(chars string) {
	params := map[string]interface{}{}
	if chars != "" {
		params["chars"] = chars
	}
	params["case_sensitive"] = false

	cmd := &EditCommand{
		Method: "find",
		ViewID: v.ID,
		Params: params,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// FindNext finds
func (v *View) FindNext(allowSame bool) {
	params := map[string]interface{}{}
	params["wrap_around"] = true
	params["allow_same"] = allowSame

	cmd := &EditCommand{
		Method: "find_next",
		ViewID: v.ID,
		Params: params,
	}
	v.xi.Conn.Notify(context.Background(), "edit", &cmd)
}

// GetContents gets
func (v *View) GetContents() string {
	params := map[string]string{
		"view_id": v.ID,
	}
	var result string
	err := v.xi.Conn.Call(context.Background(), "debug_get_contents", &params, &result)
	if err != nil {
		log.Infoln(err)
		return ""
	}
	return result
}

// PluginRPC sends
func (v *View) PluginRPC() {
	params := map[string]interface{}{}
	params["arg_one"] = true

	pluginNotification := &PluginNotification{
		Command:  "plugin_rpc",
		ViewID:   v.ID,
		Receiver: "lsp",
		RPC: &PlaceholderRPC{
			Method:  "custom_method",
			Params:  params,
			RPCType: "notification",
		},
	}
	v.xi.Conn.Notify(context.Background(), "plugin", &pluginNotification)
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

// MeasureWidthRequest is
type MeasureWidthRequest struct {
	ID     jsonrpc2.ID
	Params []*MeasureWidthParams
}

// MeasureWidthParams is
type MeasureWidthParams struct {
	ID      int      `json:"id"`
	Strings []string `json:"strings"`
}

// UpdateNotification is
type UpdateNotification struct {
	Update struct {
		Ops []struct {
			Lines []struct {
				Cursor []int  `json:"cursor"`
				Styles []int  `json:"styles"`
				Text   string `json:"text"`
			} `json:"lines"`
			N  int    `json:"n"`
			Op string `json:"op"`
		} `json:"ops"`
		Pristine bool `json:"pristine"`
	} `json:"update"`
	ViewID string `json:"view_id"`
}
