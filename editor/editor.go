package editor

import (
	"fmt"
	"log"
	"net"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/dzhou121/crane/lsp"
	xi "github.com/dzhou121/crane/xi-client"
	homedir "github.com/mitchellh/go-homedir"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// Editor is
type Editor struct {
	app             *widgets.QApplication
	window          *widgets.QMainWindow
	scence          *widgets.QGraphicsScene
	centralWidget   *widgets.QWidget
	centralSplitter *widgets.QSplitter
	signal          *editorSignal
	cursor          *widgets.QWidget
	statusLine      *StatusLine
	cache           *Cache

	cwd string

	svgsOnce sync.Once
	svgs     map[string]*SvgXML

	themeName string
	themes    []string
	theme     *xi.Theme
	bgBrush   *gui.QBrush
	fgBrush   *gui.QBrush

	topWin   *Window
	topFrame *Frame
	palette  *Palette
	popup    *Popup

	monoFont    *Font
	defaultFont *Font

	styles         map[int]*Style
	stylesRWMutext sync.RWMutex

	bufferPaths    map[string]*Buffer
	buffers        map[string]*Buffer
	buffersRWMutex sync.RWMutex

	activeWin    *Window
	winIndex     int
	wins         map[int]*Window
	winsRWMutext sync.RWMutex

	width  int
	height int

	updates chan interface{}

	xi        *xi.Xi
	lspClient *LspClient

	init     chan struct{}
	initOnce sync.Once

	states    map[int]State
	mode      int
	selection bool
	cmdArg    *CmdArg
	keymap    *Keymap
	config    *Config

	selectedBg *Color
	matchFg    *Color

	specialKeys     map[core.Qt__Key]string
	controlModifier core.Qt__KeyboardModifier
	cmdModifier     core.Qt__KeyboardModifier
	shiftModifier   core.Qt__KeyboardModifier
	altModifier     core.Qt__KeyboardModifier
	metaModifier    core.Qt__KeyboardModifier
	keyControl      core.Qt__Key
	keyCmd          core.Qt__Key
	keyAlt          core.Qt__Key
	keyShift        core.Qt__Key

	searchForward bool

	smoothScroll bool
}

type editorSignal struct {
	core.QObject
	_ func() `signal:"updateSignal"`
}

// NewEditor is
func NewEditor() (*Editor, error) {
	e := &Editor{
		updates:      make(chan interface{}, 1000),
		init:         make(chan struct{}),
		buffers:      map[string]*Buffer{},
		bufferPaths:  map[string]*Buffer{},
		wins:         map[int]*Window{},
		styles:       map[int]*Style{},
		bgBrush:      gui.NewQBrush(),
		fgBrush:      gui.NewQBrush(),
		smoothScroll: false,
		config:       loadConfig(),
		cmdArg:       &CmdArg{},
		selectedBg:   newColor(81, 154, 186, 127),
		matchFg:      newColor(81, 154, 186, 255),
	}
	e.cache = newCache(e)
	go e.startLspClient()
	e.cwd, _ = os.Getwd()
	loadKeymap(e)
	e.initSpecialKeys()
	e.states = newStates(e)
	if !e.config.Modal {
		e.mode = Insert
	} else {
		e.mode = Normal
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
		case *lsp.Location:
			if strings.HasPrefix(u.URI, "file://") {
				w := e.activeWin

				path := string(u.URI[7:])
				pos := u.Range.Start
				row := pos.Line
				col := pos.Character

				loc := &Location{
					path:   path,
					Row:    row,
					Col:    col,
					center: true,
				}
				w.openLocation(loc, true, false)
			}
		case *xi.UpdateNotification:
			e.buffersRWMutex.RLock()
			buffer, ok := e.buffers[u.ViewID]
			e.buffersRWMutex.RUnlock()
			if !ok {
				return
			}
			buffer.applyUpdate(u)
		case *xi.ConfigChanged:
			buffer, ok := e.buffers[u.ViewID]
			if !ok {
				return
			}
			buffer.setConfig(&u.Changes)
		case *xi.Themes:
			e.themes = u.Themes
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
			e.activeWin.scrollFromXi(u.Line, u.Col)
		case *xi.Style:
			e.stylesRWMutext.Lock()
			e.styles[u.ID] = &Style{
				fg: colorFromARBG(u.FgColor),
			}
			e.stylesRWMutext.Unlock()
		case *xi.Theme:
			e.theme = u
			fg := u.Theme.Foreground
			e.cursor.SetStyleSheet(fmt.Sprintf("background-color: rgba(%d, %d, %d, 0.6);", fg.R, fg.G, fg.B))
			scrollBarStyleSheet := e.getScrollbarStylesheet()

			sel := u.Theme.Selection
			e.stylesRWMutext.Lock()
			e.styles[0] = &Style{
				fg: &Color{
					R: fg.R,
					G: fg.G,
					B: fg.B,
					A: fg.A,
				},
				bg: &Color{
					R: sel.R,
					G: sel.G,
					B: sel.B,
					A: sel.A,
				},
			}
			e.stylesRWMutext.Unlock()

			e.winsRWMutext.RLock()
			defer e.winsRWMutext.RUnlock()
			for _, win := range e.wins {
				win.widget.SetStyleSheet(scrollBarStyleSheet)
				win.verticalScrollBarWidth = win.verticalScrollBar.Width()
				win.horizontalScrollBarHeight = win.horizontalScrollBar.Height()
			}
			e.palette.mainWidget.SetStyleSheet(scrollBarStyleSheet)
		}
	})
	e.xi.ClientStart(e.config.configDir)
	e.xi.SetTheme("one_dark")

	e.app = widgets.NewQApplication(0, nil)
	e.app.ConnectAboutToQuit(func() {
		fmt.Println("now quit")
		for _, win := range e.wins {
			win.saveCurrentLocation()
		}
	})
	e.initMainWindow()

	return e, nil
}

func (e *Editor) startLspClient() {
	for {
		conn, err := net.Dial("tcp", "127.0.0.1:50051")
		if err != nil {
			time.Sleep(500 * time.Millisecond)
			continue
		}
		log.Println("lsp connected")
		e.lspClient = newLspClient(e, conn)
		return
	}
}

func (e *Editor) getScrollbarStylesheet() string {
	bg := e.theme.Theme.Background
	guide := e.theme.Theme.Selection
	backgroundColor := fmt.Sprintf("rgba(%d, %d, %d, 1);", bg.R, bg.G, bg.B)
	guideColor := fmt.Sprintf("rgba(%d, %d, %d, %f);", guide.R, guide.G, guide.B, float64(guide.A)/255)
	fmt.Println(guideColor)
	scrollBarStyleSheet := fmt.Sprintf(`
			QWidget {
			    background: %s;
			}
			QAbstractScrollArea::corner {
			    background: %s;
			    border: 0px solid grey;
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
		backgroundColor, backgroundColor, backgroundColor, guideColor, backgroundColor, guideColor)
	return scrollBarStyleSheet
}

func (e *Editor) handleXiNotification(update interface{}) {
	e.updates <- update
	e.signal.UpdateSignal()
}

func (e *Editor) keyPress(event *gui.QKeyEvent) {
	key := e.convertKey(event)
	if key == "" {
		return
	}

	if e.palette.active {
		e.palette.executeKey(key)
		return
	}

	if e.popup.shown {
		if e.popup.executeKey(key) {
			return
		}
	}

	e.executeKey(key)
}

func (e *Editor) initMainWindow() {
	e.width = 800
	e.height = 600
	e.window = widgets.NewQMainWindow(nil, 0)
	dir, _ := os.Getwd()
	home, _ := homedir.Dir()
	if strings.HasPrefix(dir, home) {
		dir = strings.Replace(dir, home, "~", 1)
	}
	title := fmt.Sprintf("Crane - %s", dir)
	e.window.SetWindowTitle(title)
	e.window.SetContentsMargins(0, 0, 0, 0)
	e.window.SetMinimumSize2(e.width, e.height)
	e.window.ConnectResizeEvent(func(event *gui.QResizeEvent) {
		rect := e.window.Rect()
		e.width = rect.Width()
		e.height = rect.Height()
		e.equalWins()
		for _, w := range e.wins {
			w.view.Hide()
			w.view.Show()
		}
		e.palette.resize()
	})

	e.centralSplitter = widgets.NewQSplitter2(core.Qt__Horizontal, nil)
	e.centralSplitter.SetChildrenCollapsible(false)
	e.centralSplitter.SetStyleSheet(e.getSplitterStylesheet())
	topSplitter := widgets.NewQSplitter2(core.Qt__Horizontal, nil)
	topSplitter.SetChildrenCollapsible(false)
	topSplitter.SetStyleSheet(e.getSplitterStylesheet())
	// sideWidget := widgets.NewQWidget(nil, 0)
	// sideWidget.SetFixedWidth(50)
	// e.centralWidget.AddWidget(sideWidget)
	e.centralSplitter.AddWidget(topSplitter)

	layout := widgets.NewQVBoxLayout()
	layout.SetContentsMargins(0, 0, 0, 0)
	layout.SetSpacing(0)
	layout.AddWidget(e.centralSplitter, 1, 0)
	e.centralWidget = widgets.NewQWidget(nil, 0)
	e.centralWidget.SetLayout(layout)

	e.window.SetCentralWidget(e.centralWidget)

	e.monoFont = NewFont("Inconsolata")
	e.defaultFont = NewFont("")

	e.statusLine = newStatusLine(e)
	layout.AddWidget(e.statusLine.widget, 0, 0)

	e.topFrame = &Frame{
		width:    e.width,
		height:   e.height,
		editor:   e,
		splitter: topSplitter,
		vertical: true,
		children: []*Frame{},
	}
	frame := &Frame{
		editor: e,
		parent: e.topFrame,
	}
	e.topFrame.children = append(e.topFrame.children, frame)
	topWin := NewWindow(e, frame)
	topWin.openFile("/Users/Lulu/go/src/github.com/dzhou121/crane/editor/buffer.go")
	topSplitter.AddWidget(topWin.widget)
	e.equalWins()

	e.popup = newPopup(e)
	e.cursor = widgets.NewQWidget(nil, 0)
	e.cursor.ConnectWheelEvent(func(event *gui.QWheelEvent) {
		e.activeWin.viewWheel(event)
	})
	e.cursor.Resize2(1, 20)
	e.cursor.SetStyleSheet("background-color: rgba(0, 0, 0, 0.1);")
	e.cursor.Show()
	e.topFrame.setFocus(true)

	e.palette = newPalette(e)
	e.palette.mainWidget.SetParent(e.window)
	e.palette.mainWidget.Hide()

	e.window.Show()
	e.initOnce.Do(func() {
		close(e.init)
	})
}

func (e *Editor) getSplitterStylesheet() string {
	return `
			QSplitter::handle {
				background-color: #000;
			    image: url(images/splitter.png);
			}
			
			QSplitter::handle:horizontal {
			    width: 1px;
			}
			
			QSplitter::handle:vertical {
			    height: 1px;
			}
			
			QSplitter::handle:pressed {
			    url(images/splitter_pressed.png);
			}
	`
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
	e.topFrame.splitterResize()
}

// Run the main thread
func (e *Editor) Run() {
	widgets.QApplication_Exec()
}
