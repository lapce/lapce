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
	editor   *Editor
	children []*Frame
	parent   *Frame
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
	newFrame := &Frame{editor: f.editor}
	newWin := NewWindow(win.editor, newFrame)
	newWin.loadBuffer(win.buffer)
	newWin.row = win.row
	newWin.col = win.col
	newWin.cursorX = win.cursorX
	newWin.cursorY = win.cursorY

	parent := f.parent
	if parent != nil && parent.vertical == vertical {
		newFrame.parent = parent
		children := []*Frame{}
		for _, child := range parent.children {
			if child == f {
				children = append(children, child)
				children = append(children, newFrame)
			} else {
				children = append(children, child)
			}
		}
		parent.children = children
	} else {
		newFrame.parent = f
		frame := &Frame{
			parent: f,
			win:    win,
			editor: f.editor,
		}
		win.frame = frame
		f.children = []*Frame{}
		f.vertical = vertical
		f.win = nil
		f.children = append(f.children, frame, newFrame)
	}
	win.editor.equalWins()
	newWin.view.HorizontalScrollBar().SetValue(win.view.HorizontalScrollBar().Value())
	newWin.view.VerticalScrollBar().SetValue(win.view.VerticalScrollBar().Value())
	newWin.scrollto(newWin.col, newWin.row, false)
	f.setFocus()
}

func (f *Frame) hasChildren() bool {
	return f.children != nil && len(f.children) > 0
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

func (f *Frame) focusAbove() {
	editor := f.editor
	editor.winsRWMutext.RLock()
	defer editor.winsRWMutext.RUnlock()

	for _, win := range editor.wins {
		y := f.y - (win.frame.y + win.frame.height)
		x1 := f.x - win.frame.x
		x2 := f.x - (win.frame.x + win.frame.width)
		if y < 1 && y >= 0 && x1 >= 0 && x2 < 0 {
			win.frame.setFocus()
			return
		}
	}
}

func (f *Frame) focusBelow() {
	editor := f.editor
	editor.winsRWMutext.RLock()
	defer editor.winsRWMutext.RUnlock()

	for _, win := range editor.wins {
		y := win.frame.y - (f.y + f.height)
		x1 := f.x - win.frame.x
		x2 := f.x - (win.frame.x + win.frame.width)
		if y < 1 && y >= 0 && x1 >= 0 && x2 < 0 {
			win.frame.setFocus()
			return
		}
	}
}

func (f *Frame) focusLeft() {
	editor := f.editor
	editor.winsRWMutext.RLock()
	defer editor.winsRWMutext.RUnlock()

	for _, win := range editor.wins {
		x := f.x - (win.frame.x + win.frame.width)
		y1 := f.y - win.frame.y
		y2 := f.y - (win.frame.y + win.frame.height)
		if x < 1 && x >= 0 && y1 >= 0 && y2 < 0 {
			win.frame.setFocus()
			return
		}
	}
}

func (f *Frame) focusRight() {
	editor := f.editor
	editor.winsRWMutext.RLock()
	defer editor.winsRWMutext.RUnlock()

	for _, win := range editor.wins {
		x := win.frame.x - (f.x + f.width)
		y1 := f.y - win.frame.y
		y2 := f.y - (win.frame.y + win.frame.height)
		if x < 1 && x >= 0 && y1 >= 0 && y2 < 0 {
			win.frame.setFocus()
			return
		}
	}
}

func (f *Frame) exchange() {
	parent := f.parent
	if parent == nil {
		return
	}
	if len(parent.children) == 1 {
		parent.exchange()
		return
	}
	i := 0
	for index, child := range parent.children {
		if child == f {
			i = index
			break
		}
	}

	if i == len(parent.children)-1 {
		parent.children[i], parent.children[i-1] = parent.children[i-1], parent.children[i]
	} else {
		parent.children[i], parent.children[i+1] = parent.children[i+1], parent.children[i]
	}
	f.editor.equalWins()
	parent.children[i].setFocus()
}

func (f *Frame) setFocus() {
	if f.hasChildren() {
		f.children[0].setFocus()
		return
	}
	w := f.win
	w.view.SetFocus2()
	f.editor.activeWin = f.win
	f.editor.cursor.SetParent(f.win.view)
	f.editor.cursor.Move2(w.cursorX, w.cursorY)
	f.editor.cursor.Hide()
	f.editor.cursor.Show()
	w.buffer.xiView.Click(w.row, w.col)
}

func (f *Frame) close() *Frame {
	if f.hasChildren() {
		return nil
	}
	if f.parent == nil {
		return nil
	}
	parent := f.parent
	children := []*Frame{}
	i := 0
	for index, child := range parent.children {
		if child != f {
			children = append(children, child)
		} else {
			i = index
		}
	}
	var newFocus *Frame
	parent.children = children
	if len(children) == 0 {
		newFocus = parent.close()
	} else {
		if i > 0 {
			i--
		}
		newFocus = children[i]
	}
	win := f.win
	if win == nil {
		return newFocus
	}
	editor := win.editor
	editor.winsRWMutext.Lock()
	delete(editor.wins, win.id)
	editor.winsRWMutext.Unlock()
	win.view.Hide()
	editor.equalWins()
	if newFocus != nil {
		newFocus.setFocus()
	}
	return newFocus
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

// Window is for displaying a buffer
type Window struct {
	id      int
	editor  *Editor
	view    *widgets.QGraphicsView
	cline   *widgets.QWidget
	frame   *Frame
	buffer  *Buffer
	x       float64
	y       float64
	cursorX int
	cursorY int
	row     int
	col     int
	start   int
	end     int
}

// NewWindow creates a new window
func NewWindow(editor *Editor, frame *Frame) *Window {
	editor.winsRWMutext.Lock()
	w := &Window{
		id:     editor.winIndex,
		editor: editor,
		frame:  frame,
		view:   widgets.NewQGraphicsView(nil),
		cline:  widgets.NewQWidget(nil, 0),
	}

	w.view.ConnectEventFilter(func(watched *core.QObject, event *core.QEvent) bool {
		if event.Type() == core.QEvent__MouseButtonPress {
			mousePress := gui.NewQMouseEventFromPointer(event.Pointer())
			w.view.MousePressEvent(mousePress)
			return true
		}
		// else if event.Type() == core.QEvent__Wheel {
		// 	wheelEvent := gui.NewQWheelEventFromPointer(event.Pointer())
		// 	// delta := wheelEvent.PixelDelta()
		// 	// w.view.ScrollContentsByDefault(delta.X(), delta.Y())
		// 	// fmt.Println("scroll by", delta.X(), delta.Y())
		// 	w.view.WheelEventDefault(wheelEvent)
		// 	return true
		// }
		return w.view.EventFilterDefault(watched, event)
	})
	w.cline.SetParent(w.view)
	w.cline.SetStyleSheet(editor.getClineStylesheet())
	w.cline.SetFocusPolicy(core.Qt__NoFocus)
	w.cline.InstallEventFilter(w.view)
	w.cline.ConnectWheelEvent(func(event *gui.QWheelEvent) {
		w.view.WheelEventDefault(event)
	})
	frame.win = w
	editor.winIndex++
	editor.wins[w.id] = w
	editor.winsRWMutext.Unlock()

	// w.view.SetFrameShape(widgets.QFrame__NoFrame)
	w.view.ConnectMousePressEvent(func(event *gui.QMouseEvent) {
		editor.activeWin = w
		editor.cursor.SetParent(w.view)
		w.view.MousePressEventDefault(event)
	})
	w.view.ConnectKeyPressEvent(func(event *gui.QKeyEvent) {
		editor.activeWin = w
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
			case "L":
				w.frame.focusRight()
				return
			case "H":
				w.frame.focusLeft()
				return
			case "J":
				w.frame.focusBelow()
				return
			case "K":
				w.frame.focusAbove()
				return
			default:
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
		w.setScroll()
	})
	w.view.SetFocusPolicy(core.Qt__ClickFocus)
	w.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)
	w.view.SetParent(editor.centralWidget)
	w.view.SetCornerWidget(widgets.NewQWidget(nil, 0))
	w.view.SetObjectName("view")
	if editor.theme != nil {
		scrollBarStyleSheet := editor.getScrollbarStylesheet()
		w.view.SetStyleSheet(scrollBarStyleSheet)
	}

	return w
}

func (w *Window) update() {
	w.start = int(float64(w.view.VerticalScrollBar().Value()) / w.buffer.font.lineHeight)
	w.end = w.start + int(float64(w.frame.height)/w.buffer.font.lineHeight+1)
	b := w.buffer
	for i := w.start; i <= w.end; i++ {
		if i >= len(b.lines) {
			break
		}
		if b.lines[i] != nil && b.lines[i].invalid {
			b.lines[i].invalid = false
			b.updateLine(i)
		}
	}
}

func (w *Window) updateCline() {
	w.cline.Move2(0, w.cursorY)
}

func (w *Window) updateCursor() {
	cursor := w.editor.cursor
	cursor.Move2(w.cursorX, w.cursorY)
	cursor.Resize2(1, int(w.buffer.font.lineHeight+0.5))
	cursor.Hide()
	cursor.Show()
}

func (w *Window) setScroll() {
	w.update()
	w.updateCursorPos()
	w.updateCursor()
	w.updateCline()
	w.buffer.xiView.Scroll(w.start, w.end)
}

func (w *Window) loadBuffer(buffer *Buffer) {
	w.buffer = buffer
	w.view.SetScene(buffer.scence)
}

func (w *Window) updateCursorPos() {
	w.cursorX = int(w.x+0.5) - w.view.HorizontalScrollBar().Value()
	w.cursorY = int(w.y+0.5) - w.view.VerticalScrollBar().Value()
}

func (w *Window) updatePos() {
	b := w.buffer
	row := w.row
	col := w.col
	text := b.lines[row].text
	if col > len(text) {
		col = len(text)
		w.col = col
	}
	w.x = b.font.fontMetrics.Width(text[:col]) - 0.5
	w.y = float64(row) * b.font.lineHeight
}

func (w *Window) scrollto(col, row int, jump bool) {
	b := w.buffer
	w.row = row
	w.col = col
	w.updatePos()
	if jump {
		w.view.EnsureVisible2(
			w.x,
			w.y,
			1,
			b.font.lineHeight,
			20,
			20,
		)
	}
	w.updateCursorPos()
	w.updateCursor()
	w.updateCline()
}
