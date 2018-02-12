package editor

import (
	"fmt"

	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// Frame is
type Frame struct {
	vertical bool
	width    int
	height   int
	x        int
	y        int
	f1       *Frame
	f2       *Frame
	parent   *Frame
	vTop     *Frame
	hTop     *Frame
	win      *Window
}

func (f *Frame) split(vertical bool) {
	if f.f1 != nil || f.f2 != nil {
		// can't split again
		return
	}
	if f.vTop == nil && vertical {
		f.vTop = f
	}
	if f.hTop == nil && !vertical {
		f.hTop = f
	}
	f.vertical = vertical
	f.f1 = &Frame{
		parent: f,
		vTop:   f.vTop,
		hTop:   f.hTop,
	}
	f.f2 = &Frame{
		parent: f,
		vTop:   f.vTop,
		hTop:   f.hTop,
	}
	if vertical {
		f.f1.height = f.height
		f.f2.height = f.height
	} else {
		f.f1.width = f.width
		f.f2.width = f.width
	}
	f.equal(vertical)

	if f.win == nil {
		return
	}

	win := f.win
	f.f1.win = win
	win.frame = f.f1
	newWin := NewWindow(win.editor, f.f2)
	newWin.loadBuffer(win.buffer)
	f.win = nil

	for _, win := range win.editor.wins {
		win.view.Resize2(win.frame.width, win.frame.height)
		win.view.Move2(win.frame.x, win.frame.y)
		fmt.Println(win.frame.x, win.frame.y, win.frame.width, win.frame.height)
		win.view.Hide()
		win.view.Show()
	}

	win.view.SetFocus2()

	return
}

func (f *Frame) equal(vertical bool) {
	top := f.vTop
	if !vertical {
		top = f.hTop
	}

	value := top.width
	if !vertical {
		value = top.height
	}
	singleValue := value / top.countSplits(vertical)
	fmt.Println("single value is", singleValue)
	top.setSize(vertical, singleValue)
	top.setPos(top.x, top.y)
}

func (f *Frame) setPos(x, y int) {
	if !f.hasSplit() {
		f.x = x
		f.y = y
		// set pos
		return
	}
	if f.f1 == nil {
		f.f2.setPos(x, y)
		return
	}
	if f.f2 == nil {
		f.f1.setPos(x, y)
		return
	}
	if f.vertical {
		f.f1.setPos(x, y)
		f.f2.setPos(x+f.f1.width, y)
		return
	}
	f.f1.setPos(x, y)
	f.f2.setPos(x, y+f.f1.height)
}

func (f *Frame) setParentValue() {
	if f.parent == nil {
		return
	}
	p := f.parent
	if p.parent == nil {
		return
	}
	w1 := 0
	w2 := 0
	h1 := 0
	h2 := 0

	if p.f1 != nil {
		w1 = p.f1.width
		h1 = p.f1.height
	}
	if p.f2 != nil {
		w2 = p.f2.width
		h2 = p.f2.height
	}
	if p.vertical {
		p.width = w1 + w2
		p.height = Max(h1, h2)
	} else {
		p.width = Max(w1, w2)
		p.height = h1 + h2
	}
	p.setParentValue()
}

func (f *Frame) setSize(vertical bool, singleValue int) {
	if !f.hasSplit() {
		if vertical {
			f.width = singleValue
		} else {
			f.height = singleValue
		}
		f.setParentValue()
		// set value
		return
	}
	if f.f1 == nil {
		f.f2.setSize(vertical, singleValue)
		return
	}
	if f.f2 == nil {
		f.f1.setSize(vertical, singleValue)
		return
	}
	if f.vertical == vertical {
		f.f1.setSize(vertical, singleValue)
		f.f2.setSize(vertical, singleValue)
		return
	}
	n1 := 0
	n2 := 0
	n1 = f.f1.countSplits(vertical)
	n2 = f.f2.countSplits(vertical)
	if n1 == n2 {
		f.f1.setSize(vertical, singleValue)
		f.f2.setSize(vertical, singleValue)
		return
	}
	newsingleValue := singleValue * Max(n1, n2) / Min(n1, n2)
	if n1 > n2 {
		f.f1.setSize(vertical, singleValue)
		f.f2.setSize(vertical, newsingleValue)
		return
	}
	f.f1.setSize(vertical, newsingleValue)
	f.f2.setSize(vertical, singleValue)
}

func (f *Frame) close() {
	if f.f1 != nil || f.f2 != nil {
		// can't close frame that has children
		return
	}
	if f.parent.f1 == f {
		f.parent.f1 = nil
	} else {
		f.parent.f2 = nil
	}
	if !f.parent.hasSplit() {
		f.parent.close()
	} else {
		f.parent.equal(f.parent.vertical)
	}
}

func (f *Frame) countSplits(vertical bool) int {
	if !f.hasSplit() {
		return 1
	}
	n1 := 0
	n2 := 0
	if f.f1 != nil {
		n1 = f.f1.countSplits(vertical)
	}
	if f.f2 != nil {
		n2 = f.f2.countSplits(vertical)
	}
	if f.vertical == vertical {
		return n1 + n2
	}
	return Max(n1, n2)
}

func (f *Frame) hasSplit() bool {
	return f.f1 != nil || f.f2 != nil
}

// Window is for displaying a buffer
type Window struct {
	id     int
	editor *Editor
	view   *widgets.QGraphicsView
	frame  *Frame
	buffer *Buffer
}

// NewWindow creates a new window
func NewWindow(editor *Editor, frame *Frame) *Window {
	editor.winsRWMutext.Lock()
	w := &Window{
		id:     editor.winIndex,
		editor: editor,
		frame:  frame,
		view:   widgets.NewQGraphicsView(nil),
	}
	frame.win = w
	editor.winIndex++
	editor.wins[w.id] = w
	editor.winsRWMutext.Unlock()

	w.view.ConnectKeyPressEvent(func(event *gui.QKeyEvent) {
		if w.buffer == nil {
			return
		}
		if event.Modifiers()&core.Qt__ControlModifier > 0 {
			switch string(event.Key()) {
			case "V":
				fmt.Println("split vertical")
				w.frame.split(true)
				return
			case "S":
				fmt.Println("split horizontal")
				w.frame.split(false)
				return
			case "W":
				fmt.Println("close split")
				w.frame.close()
				return
			}
			return
		}

		switch core.Qt__Key(event.Key()) {
		case core.Qt__Key_Return, core.Qt__Key_Enter:
			w.buffer.xiView.InsertNewline()
			return
		case core.Qt__Key_Up:
			w.buffer.xiView.MoveUp()
			return
		case core.Qt__Key_Down:
			w.buffer.xiView.MoveDown()
			return
		case core.Qt__Key_Right:
			w.buffer.xiView.MoveRight()
			return
		case core.Qt__Key_Left:
			w.buffer.xiView.MoveLeft()
			return
		case core.Qt__Key_Tab, core.Qt__Key_Backtab:
			w.buffer.xiView.InsertTab()
			return
		case core.Qt__Key_Backspace:
			w.buffer.xiView.DeleteBackward()
			return
		case core.Qt__Key_Delete:
			w.buffer.xiView.DeleteForward()
			return
		case core.Qt__Key_Escape:
			return
		default:
		}
		w.buffer.xiView.Insert(event.Text())
	})
	w.view.ConnectScrollContentsBy(func(dx, dy int) {
		w.view.ScrollContentsByDefault(dx, dy)
	})
	w.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)
	w.view.SetParent(editor.centralWidget)

	return w
}

func (w *Window) loadBuffer(buffer *Buffer) {
	w.buffer = buffer
	w.view.SetScene(buffer.scence)
}
