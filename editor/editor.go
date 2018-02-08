package editor

import (
	"sync"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/widgets"
)

// Editor is
type Editor struct {
	app    *widgets.QApplication
	window *widgets.QMainWindow
	scence *widgets.QGraphicsScene
	signal *editorSignal

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
			e.view.applyUpdate(u)
		case *xi.ScrollTo:
			e.view.scrollto(u.Col, u.Line)
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
	e.height = 100
	e.window = widgets.NewQMainWindow(nil, 0)
	e.window.SetWindowTitle("Gonvim")
	e.window.SetContentsMargins(0, 0, 0, 0)
	e.window.SetMinimumSize2(e.width, e.height)

	NewView(e)

	widget := widgets.NewQWidget(nil, 0)
	widget.SetContentsMargins(0, 0, 0, 0)
	e.window.SetCentralWidget(e.view.view)
	e.window.Show()

	e.initOnce.Do(func() {
		close(e.init)
	})
}

// Run the main thread
func (e *Editor) Run() {
	widgets.QApplication_Exec()
}
