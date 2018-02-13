package editor

import (
	"fmt"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// ScenceLine is
type ScenceLine struct {
	line       *widgets.QGraphicsTextItem
	textCursor *gui.QTextCursor
	document   *gui.QTextDocument
}

// Buffer is
type Buffer struct {
	editor      *Editor
	scence      *widgets.QGraphicsScene
	scenceLines map[int]*ScenceLine
	charFormat  *gui.QTextCharFormat
	font        *Font

	nInvalidBefore int
	lines          []*Line
	nInvalidAfter  int
	revision       int
	xiView         *xi.View
}

// Color is
type Color struct {
	R int
	G int
	B int
	A int
}

// Style is
type Style struct {
	fg *Color
	bg *Color
}

func colorFromARBG(argb int) *Color {
	a := (argb >> 24) & 0xff
	r := (argb >> 16) & 0xff
	g := (argb >> 8) & 0xff
	b := argb & 0xff
	return &Color{
		A: a,
		R: r,
		G: g,
		B: b,
	}
}

// NewBuffer creates a new buffer
func NewBuffer(editor *Editor, path string) *Buffer {
	buffer := &Buffer{
		editor:      editor,
		scence:      widgets.NewQGraphicsScene(nil),
		scenceLines: map[int]*ScenceLine{},
		charFormat:  gui.NewQTextCharFormat(),
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
	buffer.scence.SetBackgroundBrush(editor.bgBrush)
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
					if b.getLine(i) != nil {
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
					text:   line.Text,
					styles: line.Styles,
					cursor: line.Cursor,
				})
			}
		case "copy", "update":
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
					for ix := startIx; ix < startIx+nCopy; ix++ {
						newLines = append(newLines, b.getLine(ix))
					}
				} else {
					jsonIx := n - nRemaining
					for ix := startIx; ix < startIx+nCopy; ix++ {
						if b.lines[ix] != nil && len(op.Lines[jsonIx].Styles) > 0 {
							b.lines[ix].styles = op.Lines[jsonIx].Styles
						}
						newLines = append(newLines, b.getLine(ix))
						jsonIx++
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
				b.scence.RemoveItem(scenceLine.line)
				delete(b.scenceLines, i)
				continue
			}
			line := b.lines[i]
			scenceLine := b.getScenceLine(i)
			textCursor := scenceLine.textCursor
			if line == nil {
				scenceLine.document.Clear()
			} else {
				if len(line.styles) < 3 {
					scenceLine.document.SetPlainText(line.text)
				} else {
					textCursor.Select(gui.QTextCursor__Document)
					start := 0
					for i := 0; i*3+2 < len(line.styles); i++ {
						start += line.styles[i*3]
						length := line.styles[i*3+1]
						styleID := line.styles[i*3+2]
						style := b.editor.getStyle(styleID)
						if style != nil {
							fg := style.fg
							b.charFormat.SetForeground(gui.NewQBrush3(gui.NewQColor3(fg.R, fg.G, fg.B, fg.A), core.Qt__SolidPattern))
							textCursor.InsertText2(string(line.text[start:start+length]), b.charFormat)
						}
						start += length
					}
				}
			}
		}
	}
	rect := b.scence.ItemsBoundingRect()
	rect.SetLeft(0)
	rect.SetTop(0)
	rect.SetWidth(rect.Width() + 20)
	b.scence.SetSceneRect(rect)
}

func (b *Buffer) getLine(ix int) *Line {
	if b.nInvalidBefore > 0 {
		fmt.Println("get line invalid before")
	}
	if ix < b.nInvalidBefore {
		return nil
	}
	ix = ix - b.nInvalidBefore
	if ix < len(b.lines) {
		return b.lines[ix]
	}
	return nil
}

func (b *Buffer) getCharFormat(styleID int) *gui.QTextCharFormat {
	style := b.editor.getStyle(styleID)
	if style == nil {
		return nil
	}
	fg := style.fg
	b.charFormat.SetForeground(gui.NewQBrush3(gui.NewQColor3(fg.R, fg.G, fg.B, fg.A), core.Qt__SolidPattern))
	return b.charFormat
}

func (b *Buffer) getScenceLine(i int) *ScenceLine {
	scenceLine, ok := b.scenceLines[i]
	if ok {
		return scenceLine
	}
	line := b.scence.AddText("", b.font.font)
	line.SetPos2(0, b.font.lineHeight*float64(i)+b.font.shift)
	line.Document().SetDocumentMargin(0)
	scenceLine = &ScenceLine{
		line:       line,
		textCursor: line.TextCursor(),
		document:   line.Document(),
	}
	b.scenceLines[i] = scenceLine
	return scenceLine
}
