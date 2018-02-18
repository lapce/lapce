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

func (b *Buffer) setNewLine(ix int, i int, winsMap map[int][]*Window) {
	wins, ok := winsMap[ix]
	if ok {
		fmt.Println("wins map ix")
		for _, win := range wins {
			fmt.Println("win scroll to")
			win.row = i
		}
	}
}

func (b *Buffer) applyUpdate(update *xi.UpdateNotification) {
	oldHeight := b.height()
	newInvalidBefore := 0
	newLines := []*Line{}
	newInvalidAfter := 0
	oldIx := 0

	bufWins := []*Window{}
	winsMap := map[int][]*Window{}
	b.editor.winsRWMutext.RLock()
	for _, win := range b.editor.wins {
		if win.buffer == b {
			bufWins = append(bufWins, win)
			if win != b.editor.activeWin {
				wins, ok := winsMap[win.row]
				if !ok {
					wins = []*Window{}
				}
				wins = append(wins, win)
				winsMap[win.row] = wins
			}
		}
	}
	b.editor.winsRWMutext.RUnlock()

	for _, op := range update.Update.Ops {
		n := op.N
		switch op.Op {
		case "invalidate":
			for ix := oldIx; ix < oldIx+n; ix++ {
				if ix >= len(b.lines) {
					newLines = append(newLines, nil)
				} else {
					line := b.lines[ix]
					newLines = append(newLines, line)
					if line != nil {
						line.invalid = true
						b.setNewLine(ix, len(newLines)-1, winsMap)
					}
				}
			}
		case "ins":
			ix := oldIx
			for _, line := range op.Lines {
				newLines = append(newLines, &Line{
					text:    line.Text,
					styles:  line.Styles,
					cursor:  line.Cursor,
					invalid: true,
				})
				b.setNewLine(ix, len(newLines)-1, winsMap)
				ix++
			}
		case "copy", "update":
			for ix := oldIx; ix < oldIx+n; ix++ {
				var line *Line
				if ix < len(b.lines) {
					line = b.lines[ix]
				}
				if line != nil && op.Op == "update" {
					opLine := op.Lines[ix-oldIx]
					line.styles = opLine.Styles
					line.cursor = opLine.Cursor
					line.invalid = true
				}
				newLines = append(newLines, line)
				if len(newLines)-1 != ix {
					if line != nil {
						line.invalid = true
					}
					b.setNewLine(ix, len(newLines)-1, winsMap)
				}
			}
			oldIx += n
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
		for i := b.height(); i < oldHeight; i++ {
			scenceLine := b.getScenceLine(i)
			b.scence.RemoveItem(scenceLine.line)
			delete(b.scenceLines, i)
		}
	}
	for _, win := range bufWins {
		win.update()
		if win != b.editor.activeWin {
			win.updatePos()
			win.updateCursorPos()
			win.updateCline()
		}
	}
	b.getScenceLine(len(b.lines) - 1)
	rect := b.scence.ItemsBoundingRect()
	rect.SetLeft(0)
	rect.SetTop(0)
	rect.SetWidth(rect.Width() + 20)
	b.scence.SetSceneRect(rect)
}

func (b *Buffer) updateLine(i int) {
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
