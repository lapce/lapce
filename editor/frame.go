package editor

import "fmt"

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
	newWin.verticalScrollBar.SetValue(win.verticalScrollValue)
	newWin.horizontalScrollBar.SetValue(win.horizontalScrollValue)
	for _, w := range win.editor.wins {
		w.scrollToCursor(w.row, w.col, true)
	}
	f.setFocus(false)
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
			win.frame.setFocus(true)
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
			win.frame.setFocus(true)
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
			win.frame.setFocus(true)
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
			win.frame.setFocus(true)
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
	parent.children[i].setFocus(true)
}

func (f *Frame) setFocus(scrollToCursor bool) {
	if f.hasChildren() {
		f.children[0].setFocus(scrollToCursor)
		return
	}
	w := f.win
	w.view.SetFocus2()
	f.editor.activeWin = f.win
	f.editor.cursor.SetParent(f.win.view)
	// f.editor.cursor.Move2(w.x, w.y)
	f.editor.cursor.Hide()
	f.editor.cursor.Show()
	if scrollToCursor {
		w.scrollToCursor(w.row, w.col, true)
	}
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
	win.widget.Hide()
	editor.equalWins()
	if newFocus != nil {
		newFocus.setFocus(true)
	}
	for _, w := range win.editor.wins {
		w.start, w.end = w.scrollRegion()
		w.gutter.Update()
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
