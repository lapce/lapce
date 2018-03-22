package editor

import (
	"fmt"
	"strconv"
	"strings"

	xi "github.com/dzhou121/crane/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// Line is
type Line struct {
	invalid bool
	text    string
	styles  []int
	cursor  []int
	current bool
	width   int
}

// Buffer is
type Buffer struct {
	editor *Editor
	scence *widgets.QGraphicsScene
	font   *Font
	widget *widgets.QWidget
	width  int
	height int
	rect   *core.QRectF
	path   string
	tabStr string

	lines     []*Line
	revision  int
	xiView    *xi.View
	maxLength int
	maxWidth  int
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

func newColor(r, g, b, a int) *Color {
	return &Color{
		A: a,
		R: r,
		G: g,
		B: b,
	}
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
		editor: editor,
		scence: widgets.NewQGraphicsScene(nil),
		lines:  []*Line{},
		font:   NewFont(),
		widget: widgets.NewQWidget(nil, 0),
		rect:   core.NewQRectF(),
		path:   path,
		tabStr: "    ",
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
	item := buffer.scence.AddWidget(buffer.widget, 0)
	item.SetPos2(0, 0)
	buffer.widget.ConnectPaintEvent(func(event *gui.QPaintEvent) {
		rect := event.M_rect()

		x := rect.X()
		y := rect.Y()
		width := rect.Width()
		height := rect.Height()

		start := y / int(buffer.font.lineHeight)

		p := gui.NewQPainter2(buffer.widget)
		bg := buffer.editor.theme.Theme.Background
		fg := buffer.editor.theme.Theme.Foreground
		p.FillRect5(x, y, width, height,
			gui.NewQColor3(bg.R, bg.G, bg.B, bg.A))

		p.SetFont(buffer.font.font)
		p.SetPen2(gui.NewQColor3(fg.R, fg.G, fg.B, fg.A))
		max := len(buffer.lines) - 1
		for i := start; i < (y+height)/int(buffer.font.lineHeight)+1; i++ {
			if i > max {
				continue
			}
			line := buffer.lines[i]
			if line == nil {
				continue
			}
			if line.text == "" {
				continue
			}
			buffer.drawLine(p, i)
		}
		defer p.DestroyQPainter()
	})
	editor.buffersRWMutex.Lock()
	editor.buffers[buffer.xiView.ID] = buffer
	editor.bufferPaths[path] = buffer
	editor.buffersRWMutex.Unlock()
	return buffer
}

func (b *Buffer) setConfig(config *xi.Config) {
	if config.TabSize > 0 {
		fmt.Println("change tab size")
		b.tabStr = ""
		for i := 0; i < config.TabSize; i++ {
			b.tabStr += " "
		}
	}
}

func (b *Buffer) drawLine(painter *gui.QPainter, index int) {
	line := b.lines[index]
	start := 0
	color := gui.NewQColor()
	for i := 0; i*3+2 < len(line.styles); i++ {
		startDiff := line.styles[i*3]
		if startDiff > 0 {
			painter.DrawText3(
				int(b.font.fontMetrics.Width(strings.Replace(string(line.text[:start]), "\t", b.tabStr, -1))+0.5),
				index*int(b.font.lineHeight)+int(b.font.shift),
				strings.Replace(string(line.text[start:start+startDiff]), "\t", b.tabStr, -1),
			)
		}

		start += startDiff
		length := line.styles[i*3+1]
		styleID := line.styles[i*3+2]
		x := b.font.fontMetrics.Width(strings.Replace(string(line.text[:start]), "\t", b.tabStr, -1))
		text := strings.Replace(string(line.text[start:start+length]), "\t", b.tabStr, -1)
		if styleID == 0 {
			theme := b.editor.theme
			if theme != nil {
				bg := theme.Theme.Selection
				color.SetRgb(bg.R, bg.G, bg.B, bg.A)
				painter.FillRect5(int(x+0.5), index*int(b.font.lineHeight),
					int(b.font.fontMetrics.Width(text)+0.5),
					int(b.font.lineHeight),
					color)
			}
		} else {
			style := b.editor.getStyle(styleID)
			if style != nil {
				fg := style.fg
				color.SetRgb(fg.R, fg.G, fg.B, fg.A)
				painter.SetPen2(color)
			}
			painter.DrawText3(int(x+0.5), index*int(b.font.lineHeight)+int(b.font.shift), text)
		}
		start += length
	}
}

func (b *Buffer) setNewLine(ix int, i int, winsMap map[int][]*Window) {
	wins, ok := winsMap[ix]
	if ok {
		for _, win := range wins {
			win.row = i
		}
	}
}

func (b *Buffer) applyUpdate(update *xi.UpdateNotification) {
	newLines := []*Line{}
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

	maxLength := 0
	maxWidth := 0
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
						if line.width > maxWidth {
							maxWidth = line.width
						}
						length := len(line.text)
						if length > maxLength {
							maxLength = length
						}
					}
				}
			}
		case "ins":
			ix := oldIx
			for _, line := range op.Lines {
				newLine := &Line{
					text:    line.Text,
					styles:  line.Styles,
					cursor:  line.Cursor,
					invalid: true,
					width:   int(b.font.fontMetrics.Width(strings.Replace(line.Text, "\t", b.tabStr, -1)) + 0.5),
				}
				newLines = append(newLines, newLine)
				b.setNewLine(ix, len(newLines)-1, winsMap)
				ix++
				if newLine.width > maxWidth {
					maxWidth = newLine.width
				}
				length := len(line.Text)
				if length > maxLength {
					maxLength = length
				}
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
				if line != nil {
					if line.width > maxWidth {
						maxWidth = line.width
					}
					length := len(line.text)
					if length > maxLength {
						maxLength = length
					}
				}
			}
			oldIx += n
		case "skip":
			oldIx += n
		default:
			fmt.Println("unknown op type", op.Op)
		}
	}

	// if len(newLines) < len(b.lines) {
	// 	for i := len(newLines); i < len(b.lines); i++ {
	// 		scenceLine := b.getScenceLine(i)
	// 		b.scence.RemoveItem(scenceLine.line)
	// 		delete(b.scenceLines, i)
	// 	}
	// }
	if len(newLines) != len(b.lines) || maxWidth != b.maxWidth {
		width := maxWidth
		height := len(newLines) * int(b.font.lineHeight)
		b.widget.Resize2(width, height)

		b.rect.SetWidth(float64(width))
		b.rect.SetHeight(float64(height))
		b.scence.SetSceneRect(b.rect)
	}

	b.lines = newLines
	b.maxWidth = maxWidth
	b.maxLength = maxLength
	b.revision++

	for _, win := range bufWins {
		win.update()
		gutterChars := len(strconv.Itoa(len(b.lines)))
		if gutterChars != win.gutterChars {
			win.gutterChars = gutterChars
			win.gutterWidth = int(float64(win.gutterChars)*win.buffer.font.width+0.5) + win.gutterPadding*2
			win.gutter.SetFixedWidth(win.gutterWidth)
		}
		if win != b.editor.activeWin {
			win.setPos(win.row, win.col, false)
		}
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
		x = int(b.font.fontMetrics.Width(strings.Replace(text[:col], "\t", b.tabStr, -1)) + 0.5)
	}
	y := row * int(b.font.lineHeight)
	return x, y
}

func (b *Buffer) updateLine(i int) {
	b.widget.Update2(0, i*int(b.font.lineHeight), 900, int(b.font.lineHeight))
}
