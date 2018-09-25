package editor

import (
	"fmt"
	"log"

	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/widgets"
)

// Frame is
type Frame struct {
	vertical bool
	cWidth   int
	cHeight  int
	width    int
	height   int
	x        int
	y        int
	splitter *widgets.QSplitter
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
	for _, w := range win.editor.wins {
		w.oldVerticalScrollValue = w.verticalScrollValue
		w.oldHorizontalScrollValue = w.horizontalScrollValue
	}
	// oldVerticalScrollValue := win.verticalScrollValue
	// oldHorizontalScrollValue := win.horizontalScrollValue
	newFrame := &Frame{editor: f.editor}
	newWin := NewWindow(win.editor, newFrame)
	newWin.loadBuffer(win.buffer)
	newWin.row = win.row
	newWin.col = win.col

	parent := f.parent
	if parent.parent == nil && len(parent.children) == 1 {
		if parent.vertical != vertical {
			if vertical {
				parent.splitter.SetOrientation(core.Qt__Horizontal)
			} else {
				parent.splitter.SetOrientation(core.Qt__Vertical)
			}
			parent.vertical = vertical
		}
	}
	if parent.vertical == vertical {
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
		parent.splitter.InsertWidget(parent.splitter.IndexOf(win.widget)+1, newWin.widget)
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
		if vertical {
			f.splitter = widgets.NewQSplitter2(core.Qt__Horizontal, nil)
		} else {
			f.splitter = widgets.NewQSplitter2(core.Qt__Vertical, nil)
		}
		f.splitter.SetChildrenCollapsible(false)
		f.splitter.SetStyleSheet(f.editor.getSplitterStylesheet())
		f.win = nil
		f.children = append(f.children, frame, newFrame)
		index := parent.splitter.IndexOf(win.widget)
		win.widget.SetParent(nil)
		f.splitter.AddWidget(win.widget)
		f.splitter.AddWidget(newWin.widget)
		parent.splitter.InsertWidget(index, f.splitter)
	}
	win.editor.equalWins()
	for _, w := range win.editor.wins {
		w.view.Hide()
		w.view.Show()
		w.verticalScrollBar.SetValue(w.oldVerticalScrollValue)
		w.horizontalScrollBar.SetValue(w.oldHorizontalScrollValue)
	}
	newWin.verticalScrollBar.SetValue(win.verticalScrollValue)
	newWin.horizontalScrollBar.SetValue(win.horizontalScrollValue)
	for _, w := range win.editor.wins {
		w.scrollToCursor(w.row, w.col, true, false, false)
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

func (f *Frame) splitterResize() {
	if !f.hasChildren() {
		return
	}

	sizes := []int{}
	for _, child := range f.children {
		if f.vertical {
			sizes = append(sizes, child.cWidth)
		} else {
			sizes = append(sizes, child.cHeight)
		}
	}
	f.splitter.SetSizes(sizes)

	for _, child := range f.children {
		child.splitterResize()
	}
}

func (f *Frame) setSize(vertical bool, singleValue int) {
	if !f.hasChildren() {
		if vertical {
			f.cWidth = singleValue
		} else {
			f.cHeight = singleValue
		}
		return
	}

	max := f.countSplits(vertical)
	if vertical {
		f.cWidth = max * singleValue
	} else {
		f.cHeight = max * singleValue
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
	f.focus(false, false, f)
}

func (f *Frame) focusBelow() {
	f.focus(false, true, f)
}

func (f *Frame) focusLeft() {
	f.focus(true, false, f)
}

func (f *Frame) focusRight() {
	f.focus(true, true, f)
}

func (f *Frame) focus(vertical bool, next bool, fromFrame *Frame) {
	if f.parent == nil {
		return
	}
	parent := f.parent
	if parent.vertical != vertical {
		parent.focus(vertical, next, fromFrame)
		return
	}
	i := 0
	for index, child := range parent.children {
		if child == f {
			i = index
			break
		}
	}
	if next {
		i++
	} else {
		i--
	}
	if i < 0 {
		parent.focus(vertical, next, fromFrame)
		return
	}
	if i >= len(parent.children) {
		parent.focus(vertical, next, fromFrame)
		return
	}
	frame := parent.children[i]
loop:
	for {
		if frame.hasChildren() {
			if frame.vertical == vertical {
				if next {
					frame = frame.children[0]
				} else {
					frame = frame.children[len(frame.children)-1]
				}
			} else {
				for _, child := range frame.children {
					if vertical {
						if child.y+child.height > fromFrame.y {
							frame = child
							continue loop
						}
					} else {
						if child.x+child.width > fromFrame.x {
							frame = child
							continue loop
						}
					}
				}
				frame = frame.children[len(frame.children)-1]
			}
		} else {
			frame.setFocus(true)
			return
		}
	}
}

func (f *Frame) changeSize(count int, vertical bool) {
	if f.parent == nil {
		fmt.Println("parent is nil")
		return
	}
	if f.parent.vertical != vertical {
		f.parent.changeSize(count, vertical)
		return
	}

	parent := f.parent
	sizes := parent.splitter.Sizes()
	i := 0
	for index, child := range parent.children {
		if child == f {
			i = index
			break
		}
	}
	fmt.Println("sizes", sizes)
	sizes[i] += count
	j := i + 1
	if i == len(parent.children)-1 {
		j = i - 1
	}
	sizes[j] -= count
	fmt.Println("new sizes", sizes)
	parent.splitter.SetSizes(sizes)
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
		child := parent.children[i]
		if child.splitter != nil {
			parent.splitter.InsertWidget(i-1, child.splitter)
		} else {
			parent.splitter.InsertWidget(i-1, child.win.widget)
		}
		parent.children[i], parent.children[i-1] = parent.children[i-1], parent.children[i]
	} else {
		child := parent.children[i+1]
		if child.splitter != nil {
			parent.splitter.InsertWidget(i, child.splitter)
		} else {
			parent.splitter.InsertWidget(i, child.win.widget)
		}
		parent.children[i], parent.children[i+1] = parent.children[i+1], parent.children[i]
	}
	parent.children[i].setFocus(true)
	f.editor.topFrame.setPos(0, 0)
}

func (f *Frame) setFocus(scrollToCursor bool) {
	if f.hasChildren() {
		f.children[0].setFocus(scrollToCursor)
		return
	}
	w := f.win
	log.Println("set focus to", w.row, w.col)
	w.view.SetFocus2()
	f.editor.activeWin = w
	f.editor.cursor.SetParent(w.view)
	f.editor.popup.view.SetParent(w.view)
	// f.editor.cursor.Move2(w.x, w.y)
	f.editor.cursor.Hide()
	f.editor.cursor.Show()
	if scrollToCursor {
		w.scrollToCursor(w.row, w.col, true, false, false)
	}
	w.buffer.xiView.Click(w.row, w.col)
	log.Println("set focus to", w.row, w.col)
	w.editor.statusLine.fileUpdate()
}

func (f *Frame) close() {
	if f.hasChildren() {
		return
	}
	if f.parent == nil {
		return
	}
	parent := f.parent
	if parent.parent == nil && len(parent.children) == 1 {
		return
	}
	win := f.win
	for _, w := range win.editor.wins {
		w.oldVerticalScrollValue = w.verticalScrollValue
		w.oldHorizontalScrollValue = w.horizontalScrollValue
	}
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
	if len(children) == 1 && parent.parent != nil {
		newSplitter := parent.parent.splitter
		child := children[0]
		if child.splitter == nil {
			newSplitter.ReplaceWidget(newSplitter.IndexOf(parent.splitter), children[0].win.widget)
			parent.children = []*Frame{}
			parent.win = children[0].win
			parent.win.frame = parent
			parent.splitter = nil
		} else {
			newSplitter.ReplaceWidget(newSplitter.IndexOf(parent.splitter), child.splitter)
			parent.children = child.children
			parent.splitter = child.splitter
			parent.vertical = child.vertical
			for _, c := range parent.children {
				c.parent = parent
			}
		}
		newFocus = parent
	} else {
		parent.children = children
		if i > 0 {
			i--
		}
		win := f.win
		win.widget.SetParent(nil)
		newFocus = children[i]
	}

	editor := f.win.editor
	editor.winsRWMutext.Lock()
	delete(editor.wins, f.win.id)
	editor.winsRWMutext.Unlock()

	f.editor.equalWins()
	for _, w := range win.editor.wins {
		w.view.Hide()
		w.view.Show()
		w.verticalScrollBar.SetValue(w.oldVerticalScrollValue)
		w.horizontalScrollBar.SetValue(w.oldHorizontalScrollValue)
	}
	newFocus.setFocus(true)
	for _, w := range f.win.editor.wins {
		w.start, w.end = w.scrollRegion()
		w.gutter.Update()
	}
	return
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
