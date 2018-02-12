package editor

import (
	"sync"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/widgets"
)

// Editor is
type Editor struct {
	app           *widgets.QApplication
	window        *widgets.QMainWindow
	scence        *widgets.QGraphicsScene
	centralWidget *widgets.QWidget
	signal        *editorSignal

	topWin   *Window
	topFrame *Frame

	buffers        map[string]*Buffer
	buffersRWMutex sync.RWMutex

	winIndex     int
	wins         map[int]*Window
	winsRWMutext sync.RWMutex

	width  int
	height int

	view *View

	updates chan interface{}

	xi *xi.Xi

	init     chan struct{}
	initOnce sync.Once
}

type editorSignal struct {
	core.QObject
	_ func() `signal:"updateSignal"`
}

// NewEditor is
func NewEditor() (*Editor, error) {
	e := &Editor{
		updates: make(chan interface{}, 1000),
		init:    make(chan struct{}),
		buffers: map[string]*Buffer{},
		wins:    map[int]*Window{},
	}

	xiClient, err := xi.New()
	if err != nil {
		return nil, err
	}
	e.xi = xiClient
	e.signal = NewEditorSignal(nil)
	e.signal.ConnectUpdateSignal(func() {
		update := <-e.updates

		switch u := update.(type) {
		case *xi.UpdateNotification:
			e.buffersRWMutex.RLock()
			buffer, ok := e.buffers[u.ViewID]
			e.buffersRWMutex.RUnlock()
			if !ok {
				return
			}
			buffer.applyUpdate(u)
		case *xi.ScrollTo:
			// e.view.scrollto(u.Col, u.Line)
		}
	})
	e.xi.HandleUpdate = e.handleUpdate
	e.xi.HandleScrollto = e.handleScrollto

	e.app = widgets.NewQApplication(0, nil)
	e.initMainWindow()

	return e, nil
}

func (e *Editor) handleUpdate(update *xi.UpdateNotification) {
	e.updates <- update
	e.signal.UpdateSignal()
}

func (e *Editor) handleScrollto(scrollto *xi.ScrollTo) {
	e.updates <- scrollto
	e.signal.UpdateSignal()
}

func (e *Editor) initMainWindow() {
	e.width = 800
	e.height = 600
	e.window = widgets.NewQMainWindow(nil, 0)
	e.window.SetWindowTitle("Gonvim")
	e.window.SetContentsMargins(0, 0, 0, 0)
	e.window.SetMinimumSize2(e.width, e.height)

	// NewView(e)

	widget := widgets.NewQWidget(nil, 0)
	widget.SetContentsMargins(0, 0, 0, 0)
	e.centralWidget = widget
	// layout := widgets.NewQHBoxLayout()
	// widget.SetLayout(layout)
	e.window.SetCentralWidget(widget)
	// layout.AddWidget(e.view.view, 0, 0)
	// layout.AddWidget(e.view.view2, 0, 0)
	// e.view.view.SetParent(widget)
	// e.view.view.Move2(0, 0)
	// e.view.view.Resize2(400, 600)
	// e.view.view2.SetParent(widget)
	// e.view.view2.Move2(400, 0)
	// e.view.view2.Resize2(400, 600)
	e.topFrame = &Frame{
		width:  e.width,
		height: e.height,
	}
	e.topWin = NewWindow(e, e.topFrame)
	e.topWin.view.Move2(0, 0)
	e.topWin.view.Resize2(e.width, e.height)
	e.topWin.loadBuffer(NewBuffer(e, "/Users/Lulu/Downloads/layout.txt"))

	e.window.Show()

	e.initOnce.Do(func() {
		close(e.init)
	})
}

// Run the main thread
func (e *Editor) Run() {
	widgets.QApplication_Exec()
}
