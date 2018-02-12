package editor

import (
	"fmt"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/widgets"
)

// Buffer is
type Buffer struct {
	scence      *widgets.QGraphicsScene
	scenceLines map[int]*widgets.QGraphicsTextItem
	font        *Font

	nInvalidBefore int
	lines          []*Line
	nInvalidAfter  int
	revision       int
	xiView         *xi.View
}

// NewBuffer creates a new buffer
func NewBuffer(editor *Editor, path string) *Buffer {
	buffer := &Buffer{
		scence:      widgets.NewQGraphicsScene(nil),
		scenceLines: map[int]*widgets.QGraphicsTextItem{},
		lines:       []*Line{},
		font:        NewFont(),
	}
	buffer.xiView, _ = editor.xi.NewView(path)
	buffer.scence.ConnectMousePressEvent(func(event *widgets.QGraphicsSceneMouseEvent) {
		scencePos := event.ScenePos()
		x := scencePos.X()
		y := scencePos.Y()
		row := y / buffer.font.lineHeight
		buffer.xiView.Click(int(row), int(x/buffer.font.width+0.5))
	})
	editor.buffersRWMutex.Lock()
	editor.buffers[buffer.xiView.ID] = buffer
	editor.buffersRWMutex.Unlock()
	return buffer
}

func (b *Buffer) height() int {
	return b.nInvalidBefore + len(b.lines) + b.nInvalidAfter
}

func (b *Buffer) applyUpdate(update *xi.UpdateNotification) {
	inval := [][]int{}
	oldHeight := b.height()
	newInvalidBefore := 0
	newLines := []*Line{}
	newInvalidAfter := 0
	oldIx := 0

	for _, op := range update.Update.Ops {
		n := op.N
		switch op.Op {
		case "invalidate":
			curLine := newInvalidBefore + len(newLines) + newInvalidAfter
			ix := curLine - b.nInvalidBefore
			if ix+n > 0 && ix < len(b.lines) {
				for i := Max(ix, 0); i < Min(ix+n, len(b.lines)); i++ {
					if b.lines[i] != nil {
						inval = append(inval, []int{i + b.nInvalidBefore, i + b.nInvalidBefore + 1})
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
			if oldIx < b.nInvalidBefore {
				nInvalid := Min(n, b.nInvalidBefore-oldIx)
				if len(newLines) == 0 {
					newInvalidBefore += nInvalid
				} else {
					newInvalidAfter += nInvalid
				}
				oldIx += nInvalid
				nRemaining -= nInvalid
			}
			if nRemaining > 0 && oldIx < b.nInvalidBefore+len(b.lines) {
				for i := 0; i < newInvalidAfter; i++ {
					newLines = append(newLines, nil)
				}
				newInvalidAfter = 0
				nCopy := Min(nRemaining, b.nInvalidBefore+len(b.lines)-oldIx)
				if oldIx != newInvalidBefore+len(newLines) || op.Op != "copy" {
					inval = append(inval, []int{newInvalidBefore + len(newLines), newInvalidBefore + len(newLines) + nCopy})
				}
				startIx := oldIx - b.nInvalidBefore
				if op.Op == "copy" {
					newLines = append(newLines, b.lines[startIx:startIx+nCopy]...)
				} else {
					for ix := startIx; ix < startIx+nCopy; ix++ {
						newLines = append(newLines, b.lines[ix])
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

	b.nInvalidBefore = newInvalidBefore
	b.lines = newLines
	b.nInvalidAfter = newInvalidAfter
	b.revision++

	if b.height() < oldHeight {
		inval = append(inval, []int{b.height(), oldHeight})
	}
	fmt.Println(inval, len(b.lines))
	for _, invalRange := range inval {
		start := invalRange[0]
		end := invalRange[1]
		for i := start; i < end; i++ {
			if i >= len(b.lines) {
				scenceLine := b.getScenceLine(i)
				b.scence.RemoveItem(scenceLine)
				delete(b.scenceLines, i)
				continue
			}
			line := b.lines[i]
			scenceLine := b.getScenceLine(i)
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
	rect := b.scence.ItemsBoundingRect()
	rect.SetLeft(0)
	rect.SetTop(0)
	b.scence.SetSceneRect(rect)
}

func (b *Buffer) getScenceLine(i int) *widgets.QGraphicsTextItem {
	scenceLine, ok := b.scenceLines[i]
	if ok {
		return scenceLine
	}
	scenceLine = b.scence.AddText("", b.font.font)
	scenceLine.SetPos2(0, b.font.lineHeight*float64(i)+b.font.shift)
	scenceLine.Document().SetDocumentMargin(0)
	b.scenceLines[i] = scenceLine
	return scenceLine
}
