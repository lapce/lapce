package editor

import (
	"fmt"
	"sync"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// Editor is
type Editor struct {
	app           *widgets.QApplication
	window        *widgets.QMainWindow
	scence        *widgets.QGraphicsScene
	centralWidget *widgets.QWidget
	signal        *editorSignal
	cursor        *widgets.QWidget

	theme   *xi.Theme
	bgBrush *gui.QBrush
	fgBrush *gui.QBrush

	topWin   *Window
	topFrame *Frame

	styles         map[int]*Style
	stylesRWMutext sync.RWMutex

	buffers        map[string]*Buffer
	buffersRWMutex sync.RWMutex

	activeWin    *Window
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
		styles:  map[int]*Style{},
		bgBrush: gui.NewQBrush(),
		fgBrush: gui.NewQBrush(),
	}

	xiClient, err := xi.New(e.handleXiNotification)
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
			if e.activeWin == nil {
				return
			}
			if e.activeWin.buffer == nil {
				return
			}
			if e.activeWin.buffer.xiView.ID != u.ViewID {
				return
			}
			e.activeWin.scrollto(u.Col, u.Line)
		case *xi.Style:
			e.stylesRWMutext.Lock()
			e.styles[u.ID] = &Style{
				fg: colorFromARBG(u.FgColor),
			}
			e.stylesRWMutext.Unlock()
		case *xi.Theme:
			e.theme = u
			fg := u.Theme.Foreground
			e.cursor.SetStyleSheet(fmt.Sprintf("background-color: rgba(%d, %d, %d, 1);", fg.R, fg.G, fg.B))
			scrollBarStyleSheet := e.getScrollbarStylesheet()
			e.winsRWMutext.RLock()
			defer e.winsRWMutext.RUnlock()
			for _, win := range e.wins {
				win.view.SetStyleSheet(scrollBarStyleSheet)
				win.cline.SetStyleSheet(e.getClineStylesheet())
			}
		}
	})
	e.xi.ClientStart()
	e.xi.SetTheme()

	e.app = widgets.NewQApplication(0, nil)
	e.initMainWindow()

	return e, nil
}

func (e *Editor) getClineStylesheet() string {
	if e.theme == nil {
		return ""
	}
	cline := e.theme.Theme.LineHighlight
	styleSheet := fmt.Sprintf("background-color: rgba(%d, %d, %d, %f);", cline.R, cline.G, cline.B, float64(cline.A)/255)
	return styleSheet
}

func (e *Editor) getScrollbarStylesheet() string {
	bg := e.theme.Theme.Background
	guide := e.theme.Theme.LineHighlight
	backgroundColor := fmt.Sprintf("rgba(%d, %d, %d, 1);", bg.R, bg.G, bg.B)
	guideColor := fmt.Sprintf("rgba(%d, %d, %d, %f);", guide.R, guide.G, guide.B, float64(guide.A)/255)
	fmt.Println(guideColor)
	scrollBarStyleSheet := fmt.Sprintf(`
			QWidget {
			    background: %s;
			}
			QWidget#view {
				border-right: 1px solid #000;
				border-bottom: 1px solid #000;
			}
			QScrollBar:horizontal {
			    border: 0px solid grey;
			    background: %s;
			    height: 10px;
			}
			QScrollBar::handle:horizontal {
			    background-color: %s;
			    min-width: 20px;
				margin: 3px 0px 3px 0px;
			}
			QScrollBar::add-line:horizontal {
			    border: 0px solid grey;
			    background: #32CC99;
			    width: 0;
			    subcontrol-position: right;
			    subcontrol-origin: margin;
			}
			QScrollBar::sub-line:horizontal {
			    border: 0px solid grey;
			    background: #32CC99;
			    width: 0px;
			    subcontrol-position: left;
			    subcontrol-origin: margin;
			}
			QScrollBar:vertical {
			    border: 0px solid;
			    background: %s;
			    width: 10px;
                margin: 0px 0px 0px 0px;
			}
			QScrollBar::handle:vertical {
			    background-color: %s;
			    min-height: 20px;
				margin: 0px 3px 0px 3px;
			}
			QScrollBar::add-line:vertical {
			    border: 0px solid grey;
			    background: #32CC99;
			    height: 0;
			    subcontrol-position: bottom;
			    subcontrol-origin: margin;
			}
			QScrollBar::sub-line:vertical {
			    border: 0px solid grey;
			    background: #32CC99;
			    height: 0px;
			    subcontrol-position: top;
			    subcontrol-origin: margin;
			}
			`,
		backgroundColor, backgroundColor, guideColor, backgroundColor, guideColor)
	return scrollBarStyleSheet
}

func (e *Editor) handleXiNotification(update interface{}) {
	e.updates <- update
	e.signal.UpdateSignal()
}

func (e *Editor) initMainWindow() {
	e.width = 800
	e.height = 600
	e.window = widgets.NewQMainWindow(nil, 0)
	e.window.SetWindowTitle("Gonvim")
	e.window.SetContentsMargins(0, 0, 0, 0)
	e.window.SetMinimumSize2(e.width, e.height)
	e.window.ConnectResizeEvent(func(event *gui.QResizeEvent) {
		rect := e.window.Rect()
		e.width = rect.Width()
		e.height = rect.Height()
		e.equalWins()
	})

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
		editor: e,
	}
	e.topWin = NewWindow(e, e.topFrame)
	e.topWin.loadBuffer(NewBuffer(e, "/Users/Lulu/xi-editor/rust/core-lib/src/rpc.rs"))
	e.equalWins()

	e.cursor = widgets.NewQWidget(nil, 0)
	e.cursor.Resize2(1, 20)
	e.cursor.SetStyleSheet("background-color: rgba(0, 0, 0, 0.5);")
	e.cursor.Show()
	e.topFrame.setFocus()
	e.window.Show()

	e.initOnce.Do(func() {
		close(e.init)
	})
}

func (e *Editor) getStyle(id int) *Style {
	e.stylesRWMutext.RLock()
	defer e.stylesRWMutext.RUnlock()
	style, ok := e.styles[id]
	if !ok {
		return nil
	}
	return style
}

func (e *Editor) equalWins() {
	itemWidth := e.width / e.topFrame.countSplits(true)
	e.topFrame.setSize(true, itemWidth)
	itemHeight := e.height / e.topFrame.countSplits(false)
	e.topFrame.setSize(false, itemHeight)
	fmt.Println("equalWins", itemWidth, itemHeight)
	e.topFrame.setPos(0, 0)
	e.organizeWins()
}

func (e *Editor) organizeWins() {
	e.winsRWMutext.RLock()
	defer e.winsRWMutext.RUnlock()
	for _, win := range e.wins {
		fmt.Println("win move and resize", win.frame.x, win.frame.y, win.frame.width, win.frame.height)
		win.view.Resize2(win.frame.width, win.frame.height)
		win.view.Move2(win.frame.x, win.frame.y)
		win.view.Hide()
		win.view.Show()
		win.cline.Resize2(win.frame.width, int(win.buffer.font.lineHeight))
		win.cline.Hide()
		win.cline.Show()
		win.setScroll()
	}
}

// Run the main thread
func (e *Editor) Run() {
	widgets.QApplication_Exec()
}
