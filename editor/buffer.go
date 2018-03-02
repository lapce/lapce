package editor

import (
	"fmt"
	"strconv"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// ScenceLine is
type ScenceLine struct {
	buffer *Buffer
	line   *widgets.QGraphicsItem
	rect   *core.QRectF
	color  *gui.QColor
	index  int
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
		row := int(y / buffer.font.lineHeight)
		col := int(x/buffer.font.width + 0.5)
		win := buffer.editor.activeWin
		win.scroll(row-win.row, col-win.col, true, false)
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

	if len(newLines) < len(b.lines) {
		for i := len(newLines); i < len(b.lines); i++ {
			scenceLine := b.getScenceLine(i)
			b.scence.RemoveItem(scenceLine.line)
			delete(b.scenceLines, i)
		}
	}

	b.nInvalidBefore = newInvalidBefore
	b.lines = newLines
	b.nInvalidAfter = newInvalidAfter
	b.revision++

	for _, win := range bufWins {
		win.update()
		win.gutterWidth = int(b.font.fontMetrics.Width(strconv.Itoa(len(b.lines)))+0.5) + win.gutterPadding*2
		win.gutter.SetFixedWidth(win.gutterWidth)
		if win != b.editor.activeWin {
			win.setPos(win.row, win.col, false)
		}
	}
	b.getScenceLine(len(b.lines) - 1)
	rect := b.scence.ItemsBoundingRect()
	rect.SetLeft(0)
	rect.SetTop(0)
	rect.SetWidth(rect.Width() + 20)
	b.scence.SetSceneRect(rect)
	for _, win := range bufWins {
		win.verticalScrollMaxValue = win.verticalScrollBar.Maximum()
		win.horizontalScrollMaxValue = win.horizontalScrollBar.Maximum()
	}
}

func (b *Buffer) getPos(row, col int) (int, int) {
	x := 0
	if row < len(b.lines) && b.lines[row] != nil {
		text := b.lines[row].text
		if col > len(text) {
			col = len(text)
		}
		x = int(b.font.fontMetrics.Width(text[:col]) + 0.5)
	}
	y := row * int(b.font.lineHeight)
	return x, y
}

func (b *Buffer) updateLine(i int) {
	line := b.lines[i]
	scenceLine := b.getScenceLine(i)
	rect := scenceLine.rect
	rect.SetWidth(b.font.width * float64(len(line.text)))
	scenceLine.line.PrepareGeometryChange()
	scenceLine.line.Update(rect)
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
	w := 1.0
	if b.lines[i] != nil {
		w = b.font.width * float64(len(b.lines[i].text))
	}
	line := widgets.NewQGraphicsItem(nil)
	b.scence.AddItem(line)
	rect := core.NewQRectF4(0, 0,
		w, b.font.lineHeight)
	line.SetPos2(1, b.font.lineHeight*float64(i))
	line.ConnectBoundingRect(func() *core.QRectF {
		return rect
	})
	scenceLine = &ScenceLine{
		buffer: b,
		line:   line,
		rect:   rect,
		index:  i,
		color:  gui.NewQColor(),
	}
	line.ConnectPaint(scenceLine.paint)
	b.scenceLines[i] = scenceLine
	return scenceLine
}

func (l *ScenceLine) paint(painter *gui.QPainter, option *widgets.QStyleOptionGraphicsItem, widget *widgets.QWidget) {
	line := l.buffer.lines[l.index]
	if line == nil {
		return
	}
	if line.text == "" {
		return
	}

	painter.SetFont(l.buffer.font.font)
	start := 0
	for i := 0; i*3+2 < len(line.styles); i++ {
		start += line.styles[i*3]
		length := line.styles[i*3+1]
		styleID := line.styles[i*3+2]
		x := l.buffer.font.fontMetrics.Width(string(line.text[:start]))
		text := string(line.text[start : start+length])
		if styleID == 0 {
			theme := l.buffer.editor.theme
			if theme != nil {
				bg := theme.Theme.Selection
				l.color.SetRgb(bg.R, bg.G, bg.B, bg.A)
				painter.FillRect5(int(x+0.5), 0,
					int(l.buffer.font.fontMetrics.Width(text)+0.5),
					int(l.buffer.font.lineHeight),
					l.color)
			}
		} else {
			style := l.buffer.editor.getStyle(styleID)
			if style != nil {
				fg := style.fg
				l.color.SetRgb(fg.R, fg.G, fg.B, fg.A)
				painter.SetPen2(l.color)
			}
			painter.DrawText3(int(x+0.5), int(l.buffer.font.shift), text)
		}
		start += length
	}

}
