package editor

import (
	"fmt"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// View displays a buffer
type View struct {
	editor      *Editor
	view        *widgets.QGraphicsView
	view2       *widgets.QGraphicsView
	scence      *widgets.QGraphicsScene
	scenceLines map[int]*widgets.QGraphicsTextItem
	cursor      *widgets.QGraphicsRectItem
	font        *Font

	nInvalidBefore int
	lines          []*Line
	nInvalidAfter  int
	revision       int

	xiView *xi.View
}

// Line is
type Line struct {
	invalid bool
	text    string
	styles  []int
	cursor  []int
}

// NewView creates a new view
func NewView(e *Editor) *View {
	view := &View{
		editor:      e,
		font:        NewFont(),
		view:        widgets.NewQGraphicsView(nil),
		scence:      widgets.NewQGraphicsScene(nil),
		scenceLines: map[int]*widgets.QGraphicsTextItem{},
		lines:       []*Line{},
	}
	e.view = view

	view2 := widgets.NewQGraphicsView(nil)
	view2.SetScene(view.scence)
	view.view2 = view2

	view.view.ConnectKeyPressEvent(func(event *gui.QKeyEvent) {
		if view.xiView == nil {
			return
		}
		if event.Modifiers()&core.Qt__ControlModifier > 0 {
			switch string(event.Key()) {
			case "V":
				fmt.Println("split vertical")
			}
			return
		}

		switch core.Qt__Key(event.Key()) {
		case core.Qt__Key_Return, core.Qt__Key_Enter:
			view.xiView.InsertNewline()
			return
		case core.Qt__Key_Up:
			view.xiView.MoveUp()
			return
		case core.Qt__Key_Down:
			view.xiView.MoveDown()
			return
		case core.Qt__Key_Right:
			view.xiView.MoveRight()
			return
		case core.Qt__Key_Left:
			view.xiView.MoveLeft()
			return
		case core.Qt__Key_Tab, core.Qt__Key_Backtab:
			view.xiView.InsertTab()
			return
		case core.Qt__Key_Backspace:
			view.xiView.DeleteBackward()
			return
		case core.Qt__Key_Delete:
			view.xiView.DeleteForward()
			return
		case core.Qt__Key_Escape:
			return
		default:
		}
		view.xiView.Insert(event.Text())
	})
	view.scence.ConnectMousePressEvent(func(event *widgets.QGraphicsSceneMouseEvent) {
		scencePos := event.ScenePos()
		x := scencePos.X()
		y := scencePos.Y()
		row := y / view.font.lineHeight
		view.xiView.Click(int(row), int(x/view.font.width+0.5))
	})
	view.view.ConnectScrollContentsBy(func(dx, dy int) {
		view.view.ScrollContentsByDefault(dx, dy)

		fmt.Println(view.view.VerticalScrollBar().Value(), view.view.HorizontalScrollBar().Value())
	})
	view.view.SetScene(view.scence)
	view.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)

	go func() {
		<-e.init
		view.xiView, _ = e.xi.NewView("/Users/Lulu/Downloads/layout.txt")
	}()
	return view
}

func (v *View) scrollto(col, row int) {
	// if v.cursor == nil {
	// 	v.cursor = v.scence.AddRect2(
	// 		0,
	// 		0,
	// 		1,
	// 		v.font.lineHeight,
	// 		gui.NewQPen(),
	// 		gui.NewQBrush3(gui.NewQColor3(0, 0, 0, 255), core.Qt__SolidPattern))
	// }
	// if row >= len(v.lines) {
	// 	row = len(v.lines) - 1
	// }
	// v.cursor.SetPos2(
	// 	v.font.fontMetrics.Width(v.lines[row].text[:col])-0.5,
	// 	)
	// v.view.EnsureVisible3(v.cursor, 20, 20)

	v.view.EnsureVisible2(
		v.font.fontMetrics.Width(v.lines[row].text[:col])-0.5,
		float64(row)*v.font.lineHeight,
		1,
		v.font.lineHeight,
		20,
		20,
	)
}

func (v *View) height() int {
	return v.nInvalidBefore + len(v.lines) + v.nInvalidAfter
}

func (v *View) applyUpdate(update *xi.UpdateNotification) {
	inval := [][]int{}
	oldHeight := v.height()
	newInvalidBefore := 0
	newLines := []*Line{}
	newInvalidAfter := 0
	oldIx := 0

	for _, op := range update.Update.Ops {
		n := op.N
		switch op.Op {
		case "invalidate":
			curLine := newInvalidBefore + len(newLines) + newInvalidAfter
			ix := curLine - v.nInvalidBefore
			if ix+n > 0 && ix < len(v.lines) {
				for i := Max(ix, 0); i < Min(ix+n, len(v.lines)); i++ {
					if v.lines[i] != nil {
						inval = append(inval, []int{i + v.nInvalidBefore, i + v.nInvalidBefore + 1})
					}
				}
			}
			if len(newLines) == 0 {
				newInvalidBefore += n
			} else {
				newInvalidAfter += n
			}
		case "ins":
			for i := 0; i < newInvalidAfter; i++ {
				newLines = append(newLines, nil)
			}
			newInvalidAfter = 0
			inval = append(inval, []int{newInvalidBefore + len(newLines), newInvalidBefore + len(newLines) + n})
			for _, line := range op.Lines {
				newLines = append(newLines, &Line{
					text: line.Text,
				})
			}
		case "copy", "update":
			n := op.N
			nRemaining := n
			if oldIx < v.nInvalidBefore {
				nInvalid := Min(n, v.nInvalidBefore-oldIx)
				if len(newLines) == 0 {
					newInvalidBefore += nInvalid
				} else {
					newInvalidAfter += nInvalid
				}
				oldIx += nInvalid
				nRemaining -= nInvalid
			}
			if nRemaining > 0 && oldIx < v.nInvalidBefore+len(v.lines) {
				for i := 0; i < newInvalidAfter; i++ {
					newLines = append(newLines, nil)
				}
				newInvalidAfter = 0
				nCopy := Min(nRemaining, v.nInvalidBefore+len(v.lines)-oldIx)
				if oldIx != newInvalidBefore+len(newLines) || op.Op != "copy" {
					inval = append(inval, []int{newInvalidBefore + len(newLines), newInvalidBefore + len(newLines) + nCopy})
				}
				startIx := oldIx - v.nInvalidBefore
				if op.Op == "copy" {
					newLines = append(newLines, v.lines[startIx:startIx+nCopy]...)
				} else {
					for ix := startIx; ix < startIx+nCopy; ix++ {
						newLines = append(newLines, v.lines[ix])
					}
				}
				oldIx += nCopy
				nRemaining -= nCopy
			}
			if len(newLines) == 0 {
				newInvalidBefore += nRemaining
			} else {
				newInvalidAfter += nRemaining
			}
			oldIx += nRemaining
		case "skip":
			oldIx += n
		default:
			fmt.Println("unknown op type", op.Op)
		}
	}

	v.nInvalidBefore = newInvalidBefore
	v.lines = newLines
	v.nInvalidAfter = newInvalidAfter
	v.revision++

	if v.height() < oldHeight {
		inval = append(inval, []int{v.height(), oldHeight})
	}
	fmt.Println(inval, len(v.lines))
	for _, invalRange := range inval {
		start := invalRange[0]
		end := invalRange[1]
		for i := start; i < end; i++ {
			if i >= len(v.lines) {
				scenceLine := v.getScenceLine(i)
				v.scence.RemoveItem(scenceLine)
				delete(v.scenceLines, i)
				continue
			}
			line := v.lines[i]
			scenceLine := v.getScenceLine(i)
			textCursor := scenceLine.TextCursor()
			if line == nil {
				textCursor.Document().Clear()
			} else {
				textCursor.Document().SetPlainText(line.text)
				// if len(line.text) > 3 {
				// 	textCursor.SetPosition(1, gui.QTextCursor__MoveAnchor)
				// 	textCursor.SetPosition(3, gui.QTextCursor__KeepAnchor)
				// 	charFormat := gui.NewQTextCharFormat()
				// 	// charFormat.SetBackground(gui.NewQBrush3(gui.NewQColor3(100, 100, 100, 255), core.Qt__SolidPattern))
				// 	// charFormat.SetFontItalic(true)
				// 	// charFormat.SetFontUnderline(true)
				// 	charFormat.SetForeground(gui.NewQBrush3(gui.NewQColor3(100, 100, 100, 255), core.Qt__SolidPattern))
				// 	textCursor.SetCharFormat(charFormat)
				// 	// fmt.Println(scenceLine.TextCursor().SelectionStart())
				// 	// fmt.Println(scenceLine.TextCursor().SelectionEnd())
				// }
			}
		}
	}
	rect := v.scence.ItemsBoundingRect()
	rect.SetLeft(0)
	rect.SetTop(0)
	v.scence.SetSceneRect(rect)
}

func (v *View) getScenceLine(i int) *widgets.QGraphicsTextItem {
	scenceLine, ok := v.scenceLines[i]
	if ok {
		return scenceLine
	}
	scenceLine = v.scence.AddText("", v.font.font)
	scenceLine.SetPos2(0, v.font.lineHeight*float64(i)+v.font.shift)
	scenceLine.Document().SetDocumentMargin(0)
	v.scenceLines[i] = scenceLine
	return scenceLine
}
