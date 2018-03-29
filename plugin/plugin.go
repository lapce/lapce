package plugin

import (
	"context"
	"encoding/json"
	"log"
	"runtime/debug"

	"github.com/sourcegraph/jsonrpc2"
)

// Plugin is
type Plugin struct {
	Views      map[string]*View
	conn       *jsonrpc2.Conn
	Stop       chan struct{}
	handleFunc HandleFunc
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

// BufferInfo is
type BufferInfo struct {
	BufSize  int      `json:"buf_size"`
	BufferID int      `json:"buffer_id"`
	Config   *Config  `json:"config"`
	NbLines  int      `json:"nb_lines"`
	Path     string   `json:"path"`
	Rev      uint64   `json:"rev"`
	Syntax   string   `json:"syntax"`
	Views    []string `json:"views"`
}

// Initialization is
type Initialization struct {
	BufferInfo []*BufferInfo `json:"buffer_info"`
	PluginID   int           `json:"plugin_id"`
}

// Update is
type Update struct {
	Author string `json:"author"`
	Delta  struct {
		BaseLen int `json:"base_len"`
		Els     []struct {
			Copy   []int  `json:"copy,omitempty"`
			Insert string `json:"insert,omitempty"`
		} `json:"els"`
	} `json:"delta"`
	EditType string `json:"edit_type"`
	NewLen   int    `json:"new_len"`
	Rev      uint64 `json:"rev"`
	ViewID   string `json:"view_id"`
}

// HandleFunc is
type HandleFunc func(req interface{})

// NewPlugin is
func NewPlugin() *Plugin {
	p := &Plugin{
		Stop:  make(chan struct{}),
		Views: map[string]*View{},
	}
	p.conn = jsonrpc2.NewConn(context.Background(), NewStdinoutStream(), p)
	return p
}

// SetHandleFunc is
func (p *Plugin) SetHandleFunc(handleFunc HandleFunc) {
	p.handleFunc = handleFunc
}

// Handle incoming
func (p *Plugin) Handle(ctx context.Context, conn *jsonrpc2.Conn, req *jsonrpc2.Request) {
	defer func() {
		if r := recover(); r != nil {
			log.Println("handle error", r, string(debug.Stack()))
		}
	}()
	params, err := req.Params.MarshalJSON()
	if err != nil {
		log.Println(err)
		return
	}
	log.Println("now handle", req.ID, req.Method, string(params))
	switch req.Method {
	case "initialize":
		var initialization *Initialization
		err := json.Unmarshal(params, &initialization)
		if err != nil {
			log.Println(err)
			return
		}
		for _, buf := range initialization.BufferInfo {
			p.initBuf(buf)
		}
		if p.handleFunc != nil {
			p.handleFunc(initialization)
		}
	case "new_buffer":
		var initialization *Initialization
		err := json.Unmarshal(params, &initialization)
		if err != nil {
			log.Println(err)
			return
		}
		for _, buf := range initialization.BufferInfo {
			p.initBuf(buf)
		}
		if p.handleFunc != nil {
			p.handleFunc(initialization)
		}
	case "update":
		var update *Update
		err := json.Unmarshal(params, &update)
		if err != nil {
			log.Println(err)
			return
		}
		// p.Views[update.ViewID].LineCache.ApplyUpdate(update)
		if p.handleFunc != nil {
			p.handleFunc(update)
		}
	}
	log.Println("handle done")
}

func (p *Plugin) initBuf(buf *BufferInfo) {
	lineCache := &LineCache{
		ViewID: buf.Views[0],
	}
	for _, viewID := range buf.Views {
		p.Views[viewID] = &View{
			ID:        viewID,
			Path:      buf.Path,
			LineCache: lineCache,
		}
	}
}
