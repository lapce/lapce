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
	children []*Frame
	parent   *Frame
	vTop     *Frame
	hTop     *Frame
	win      *Window
}

func (f *Frame) split(vertical bool) {
	if f.hasChildren() {
		fmt.Println("split has children already")
		return
	}
	win := f.win
	if win == nil {
		return
	}
	newFrame := &Frame{}
	newWin := NewWindow(win.editor, newFrame)
	newWin.loadBuffer(win.buffer)

	parent := f.parent
	if parent != nil && parent.vertical == vertical {
		newFrame.parent = parent
		parent.children = append(parent.children, newFrame)
	} else {
		newFrame.parent = f
		frame := &Frame{
			parent: f,
			win:    win,
		}
		win.frame = frame
		f.children = []*Frame{}
		f.vertical = vertical
		f.win = nil
		f.children = append(f.children, frame, newFrame)
	}
	win.editor.equalWins()
	win.view.SetFocus2()
}

func (f *Frame) hasChildren() bool {
	return f.children != nil && len(f.children) > 0
}

func (f *Frame) splitOld(vertical bool) {
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
	win.editor.organizeWins()
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
	f.x = x
	f.y = y
	if !f.hasChildren() {
		fmt.Println("set x y", x, y)
		return
	}

	for _, child := range f.children {
		child.setPos(x, y)
		if f.vertical {
			x += child.width
		} else {
			y += child.height
		}
	}
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
	if !f.hasChildren() {
		if vertical {
			f.width = singleValue
		} else {
			f.height = singleValue
		}
		return
	}

	max := f.countSplits(vertical)
	if vertical {
		f.width = max * singleValue
	} else {
		f.height = max * singleValue
	}

	if f.vertical == vertical {
		for _, child := range f.children {
			child.setSize(vertical, singleValue)
		}
		return
	}

	for _, child := range f.children {
		n := child.countSplits(vertical)
		child.setSize(vertical, singleValue*max/n)
	}
}

func (f *Frame) exchange() {
	parent := f.parent
	if parent == nil {
		return
	}
	var newFrame *Frame
	if parent.f1 == nil || parent.f2 == nil {
		parent.exchange()
	} else {
		if f == parent.f1 {
			parent.f1, parent.f2 = parent.f2, parent.f1
			newFrame = parent.f1
		} else {
			if parent.parent == nil {
				newFrame = parent.f1
				parent.f1, parent.f2 = parent.f2, parent.f1
			} else {
				if parent.parent.vertical == parent.vertical {
					if parent.parent.f1 == parent && !parent.parent.f2.hasSplit() {
						parent.f2, parent.parent.f2 = parent.parent.f2, parent.f2
					}
				} else {
					parent.parent.exchange()
				}
			}
			newFrame = parent.f2
		}
	}
	if f.win == nil {
		return
	}
	editor := f.win.editor
	editor.topFrame.equal(true)
	editor.topFrame.equal(false)
	editor.organizeWins()
	if newFrame != nil {
		newFrame.setFocus()
	} else {
		f.setFocus()
	}
}

func (f *Frame) setFocus() {
	if !f.hasSplit() {
		f.win.view.SetFocus2()
		return
	}
	if f.f1 != nil {
		f.f1.setFocus()
		return
	}
	f.f2.setFocus()
}

func (f *Frame) close() {
	if f.hasChildren() {
		return
	}
	if f.parent == nil {
		return
	}
	parent := f.parent
	children := []*Frame{}
	for _, child := range parent.children {
		if child != f {
			children = append(children, child)
		}
	}
	parent.children = children
	if len(children) == 0 {
		parent.close()
	}
	win := f.win
	if win == nil {
		return
	}
	editor := win.editor
	editor.winsRWMutext.Lock()
	delete(editor.wins, win.id)
	editor.winsRWMutext.Unlock()
	win.view.Hide()
	editor.equalWins()
}

func (f *Frame) closeOld() {
	if f.f1 != nil || f.f2 != nil {
		// can't close frame that has children
		return
	}

	parent := f.parent
	if parent == nil {
		return
	}

	if f == parent.f1 {
		parent.f1 = nil
	} else {
		parent.f2 = nil
	}
	if !parent.hasSplit() {
		parent.close()
	}
	if f.win == nil {
		return
	}
	editor := f.win.editor
	editor.topFrame.equal(true)
	editor.topFrame.equal(false)
	editor.winsRWMutext.Lock()
	delete(editor.wins, f.win.id)
	editor.winsRWMutext.Unlock()
	f.win.view.Hide()
	f.win.editor.organizeWins()
}

func (f *Frame) countSplits(vertical bool) int {
	if !f.hasChildren() {
		return 1
	}
	n := 0
	if f.vertical == vertical {
		for _, child := range f.children {
			n += child.countSplits(vertical)
		}
	} else {
		for _, child := range f.children {
			v := child.countSplits(vertical)
			if v > n {
				n = v
			}
		}
	}
	return n
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
			case "X":
				w.frame.exchange()
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
