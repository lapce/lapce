package editor

import (
	"fmt"
	"strconv"
	"strings"
	"sync"
	"time"

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
	editor         *Editor
	scence         *widgets.QGraphicsScene
	font           *Font
	widget         *widgets.QWidget
	width          int
	height         int
	rect           *core.QRectF
	path           string
	tabStr         string
	gotFirstUpdate bool
	pristine       bool
	inited         chan struct{}
	initOnce       sync.Once

	lines    []*Line
	newLines []*Line
	revision int
	xiView   *xi.View
	maxWidth int
}

// Color is
type Color struct {
	R int `json:"a"`
	G int `json:"b"`
	B int `json:"g"`
	A int `json:"r"`
}

// String is
func (c *Color) String() string {
	return fmt.Sprintf("rgba(%d, %d, %d, %f)", c.R, c.G, c.B, float64(c.A)/255)
}

// Hex is
func (c *Color) Hex() string {
	return fmt.Sprintf("#%02x%02x%02x", uint8(c.R), uint8(c.G), uint8(c.B))
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
		editor:   editor,
		scence:   widgets.NewQGraphicsScene(nil),
		lines:    []*Line{},
		newLines: []*Line{},
		font:     editor.monoFont,
		widget:   widgets.NewQWidget(nil, 0),
		rect:     core.NewQRectF(),
		path:     path,
		tabStr:   "    ",
		pristine: true,
		inited:   make(chan struct{}),
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
	buffer.scence.ConnectKeyPressEvent(buffer.editor.keyPress)
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
			buffer.drawLine(p, buffer.font, i, i*int(buffer.font.lineHeight), 0)
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
		b.tabStr = ""
		for i := 0; i < config.TabSize; i++ {
			b.tabStr += " "
		}
	}
}

func (b *Buffer) drawLine(painter *gui.QPainter, font *Font, index int, y int, padding int) {
	if index < 0 || index >= len(b.lines) {
		return
	}
	line := b.lines[index]
	if line == nil {
		return
	}
	start := 0
	color := gui.NewQColor()
	for i := 0; i*3+2 < len(line.styles); i++ {
		startDiff := line.styles[i*3]
		if startDiff > 0 {
			painter.DrawText3(
				padding+int(font.fontMetrics.Size(0, strings.Replace(string(line.text[:start]), "\t", b.tabStr, -1), 0, 0).Rwidth()+0.5),
				y+int(font.shift),
				strings.Replace(string(line.text[start:start+startDiff]), "\t", b.tabStr, -1),
			)
		}

		start += startDiff
		length := line.styles[i*3+1]
		styleID := line.styles[i*3+2]
		x := font.fontMetrics.Size(0, strings.Replace(string(line.text[:start]), "\t", b.tabStr, -1), 0, 0).Rwidth()
		text := strings.Replace(string(line.text[start:start+length]), "\t", b.tabStr, -1)
		if styleID == 0 {
			theme := b.editor.theme
			if theme != nil {
				bg := theme.Theme.Selection
				color.SetRgb(bg.R, bg.G, bg.B, bg.A)
				painter.FillRect5(int(x+0.5), y,
					int(font.fontMetrics.Size(0, text, 0, 0).Rwidth()+0.5),
					int(font.lineHeight),
					color)
			}
		} else {
			style := b.editor.getStyle(styleID)
			if style != nil {
				fg := style.fg
				color.SetRgb(fg.R, fg.G, fg.B, fg.A)
				painter.SetPen2(color)
			}
			painter.DrawText3(padding+int(x+0.5), y+int(font.shift), text)
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

func (b *Buffer) updateScrollInBackground() {
	num := len(b.lines)
	fmt.Println("num of lines", num)
	height := 50
	i := 0
	for {
		fmt.Println("update ", i, i+height)
		time.Sleep(500 * time.Millisecond)
		b.xiView.Scroll(i, i+height)
		i += height
		if i > num {
			return
		}
	}
}

func (b *Buffer) applyUpdate(update *xi.UpdateNotification) {
	// start := time.Now()
	// defer func() {
	// 	fmt.Println((time.Now().Nanosecond() - start.Nanosecond()) / 1e6)
	// }()
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

	maxWidth := 0
	oldIx := 0
	newIx := 0
	// text := ""
	// oldText := ""
	// contentChanges := []*lsp.ContentChange{}
	// cursors := []int{}
	// row := 0
	for _, op := range update.Update.Ops {
		n := op.N
		switch op.Op {
		case "invalidate":
			for ix := oldIx; ix < oldIx+n; ix++ {
				var line *Line
				if ix < len(b.lines) {
					line = b.lines[ix]
				}
				if newIx < len(b.newLines) {
					b.newLines[newIx] = line
				} else {
					b.newLines = append(b.newLines, line)
				}
				if line != nil {
					line.invalid = true
					b.setNewLine(ix, newIx, winsMap)
					// text += line.text
					if line.width > maxWidth {
						maxWidth = line.width
					}
				}
				newIx++
			}
		case "ins":
			ix := oldIx
			for _, line := range op.Lines {
				// if len(line.Cursor) > 0 {
				// 	row = newIx
				// 	cursors = append(cursors, line.Cursor...)
				// }
				newLine := &Line{
					text:    line.Text,
					styles:  line.Styles,
					cursor:  line.Cursor,
					invalid: true,
					width:   int(b.font.fontMetrics.Size(0, strings.Replace(line.Text, "\t", b.tabStr, -1), 0, 0).Rwidth() + 0.5),
				}
				if newIx < len(b.newLines) {
					b.newLines[newIx] = newLine
				} else {
					b.newLines = append(b.newLines, newLine)
				}
				b.setNewLine(ix, newIx, winsMap)
				// text += line.Text
				if newLine.width > maxWidth {
					maxWidth = newLine.width
				}
				ix++
				newIx++
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
				if newIx < len(b.newLines) {
					b.newLines[newIx] = line
				} else {
					b.newLines = append(b.newLines, line)
				}
				if newIx != ix {
					if line != nil {
						line.invalid = true
					}
					b.setNewLine(ix, newIx, winsMap)
				}
				if line != nil {
					if line.width > maxWidth {
						maxWidth = line.width
					}
				}
				newIx++
			}
			oldIx += n
		case "skip":
			// oldText = ""
			// for i := oldIx; i < oldIx+n; i++ {
			// 	oldText += b.lines[i].text
			// }
			// if oldText != text {
			// 	contentChange := &lsp.ContentChange{
			// 		Range: &lsp.Range{
			// 			Start: &lsp.Position{oldIx, 0},
			// 			End:   &lsp.Position{oldIx + n, 0},
			// 		},
			// 		Text: text,
			// 	}
			// 	contentChanges = append(contentChanges, contentChange)
			// }
			oldIx += n
			// text = ""
		default:
			fmt.Println("unknown op type", op.Op)
		}
	}
	// b.xiView.PluginRPC()
	// if b.revision > 0 && len(contentChanges) > 0 {
	// 	if b.editor.mode == Insert && len(contentChanges) == 1 && len(cursors) == 1 {
	// 		col := cursors[0]
	// 		if col > 0 {
	// 			if utfClass(rune(b.newLines[row].text[col-1])) == 2 {
	// 				if b.editor.lspClient != nil {
	// 					// go b.editor.lspClient.completion(b, row, col)
	// 				}
	// 			}
	// 		}
	// 	}
	// }

	if newIx < len(b.newLines) {
		b.newLines = b.newLines[:newIx]
	}

	if len(b.newLines) != len(b.lines) || maxWidth != b.maxWidth {
		width := maxWidth
		height := len(b.newLines) * int(b.font.lineHeight)
		b.width = width
		b.widget.SetFixedSize2(width, height)

		b.rect.SetWidth(float64(width))
		b.rect.SetHeight(float64(height))
		b.scence.SetSceneRect(b.rect)
		// for _, win := range bufWins {
		// 	win.view.UpdateGeometry()
		// }
	}

	b.lines, b.newLines = b.newLines, b.lines
	b.maxWidth = maxWidth
	b.revision++

	if !b.gotFirstUpdate {
		b.gotFirstUpdate = true
		b.initOnce.Do(func() {
			close(b.inited)
		})
		// go b.updateScrollInBackground()
	}

	if update.Update.Pristine != b.pristine {
		b.pristine = update.Update.Pristine
		b.editor.statusLine.fileUpdate()
	}

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
		x = int(b.font.fontMetrics.Size(0, strings.Replace(text[:col], "\t", b.tabStr, -1), 0, 0).Rwidth() + 0.5)
	}
	y := row * int(b.font.lineHeight)
	return x, y
}

func (b *Buffer) updateLine(i int) {
	b.widget.Update2(0, i*int(b.font.lineHeight), b.width, int(b.font.lineHeight))
}
