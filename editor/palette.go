package editor

import (
	"fmt"
	"sort"
	"strings"
	"unicode"

	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

//
const (
	PaletteStr = iota
	PaletteFolder
	PaletteCmd
)

//
const (
	PaletteNone    = ":none"
	PaletteCommand = ":"
	PaletteLine    = "#"
	PaletteFile    = ""
	PaletteThemes  = ":themes"
)

// Palette is
type Palette struct {
	editor      *Editor
	mainWidget  *widgets.QWidget
	input       *widgets.QWidget
	view        *widgets.QGraphicsView
	scence      *widgets.QGraphicsScene
	widget      *widgets.QWidget
	rect        *core.QRectF
	font        *Font
	active      bool
	items       []*PaletteItem
	activeItems []*PaletteItem
	index       int
	cmds        map[string]Command
	inputText   string
	inputIndex  int

	inputType string

	oldRow int
	oldCol int

	width        int
	padding      int
	inputHeight  int
	viewHeight   int
	scenceHeight int
	x            int

	selectedBg *Color
	matchFg    *Color
}

// PaletteItem is
type PaletteItem struct {
	description   string
	cmd           func()
	itemType      int
	n             int // the number of execute times
	score         int
	matches       []int
	lineNumber    int
	line          *Line
	stayInPalette bool
}

func newPalette(editor *Editor) *Palette {
	p := &Palette{
		editor:     editor,
		mainWidget: widgets.NewQWidget(nil, 0),
		input:      widgets.NewQWidget(nil, 0),
		scence:     widgets.NewQGraphicsScene(nil),
		view:       widgets.NewQGraphicsView(nil),
		widget:     widgets.NewQWidget(nil, 0),
		rect:       core.NewQRectF(),
		font:       editor.defaultFont,

		width:        600,
		padding:      12,
		inputHeight:  -1,
		viewHeight:   -1,
		scenceHeight: -1,
		inputType:    PaletteNone,

		selectedBg: newColor(81, 154, 186, 127),
		matchFg:    newColor(81, 154, 186, 255),
	}
	p.initCmds()

	layout := widgets.NewQVBoxLayout()
	layout.SetContentsMargins(0, 0, 0, 0)
	layout.SetSpacing(0)
	layout.SetSizeConstraint(widgets.QLayout__SetMinAndMaxSize)
	layout.AddWidget(p.input, 0, 0)
	layout.AddWidget(p.view, 0, 0)
	p.mainWidget.SetContentsMargins(0, 0, 0, 0)
	p.mainWidget.SetLayout(layout)
	p.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)
	p.view.SetHorizontalScrollBarPolicy(core.Qt__ScrollBarAlwaysOff)
	// p.view.SetCornerWidget(widgets.NewQWidget(nil, 0))
	p.view.SetFrameStyle(0)
	p.scence.AddWidget(p.widget, 0).SetPos2(0, 0)
	p.view.SetScene(p.scence)
	p.widget.ConnectPaintEvent(p.paint)
	p.input.ConnectPaintEvent(p.paintInput)

	shadow := widgets.NewQGraphicsDropShadowEffect(nil)
	shadow.SetBlurRadius(20)
	shadow.SetColor(gui.NewQColor3(0, 0, 0, 255))
	shadow.SetOffset3(0, 2)
	p.mainWidget.SetGraphicsEffect(shadow)
	return p
}

func (p *Palette) resize() {
	x := (p.editor.width - p.width) / 2
	if p.x != x {
		p.x = x
		p.mainWidget.Move2(x, 0)
	}
	inputHeight := int(p.font.lineHeight) + (p.padding/2)*2
	if p.inputHeight != inputHeight {
		p.input.SetFixedSize2(p.width, inputHeight)
		p.inputHeight = inputHeight
	}

	viewMaxHeight := int(float64(p.editor.height)*0.382+0.5) - inputHeight
	max := viewMaxHeight/int(p.font.lineHeight) + 1
	n := len(p.activeItems)
	if n > max {
		n = max
	}
	viewHeight := n * int(p.font.lineHeight)
	if viewHeight != p.viewHeight {
		p.viewHeight = viewHeight
		p.view.SetFixedSize2(p.width, viewHeight)
	}
	scenceHeight := len(p.activeItems) * int(p.font.lineHeight)
	if p.scenceHeight != scenceHeight {
		p.scenceHeight = scenceHeight
		scenceWidth := p.width
		if scenceHeight > viewHeight {
			scenceWidth -= 16
		}
		p.widget.Resize2(scenceWidth+10, scenceHeight)
		p.rect.SetWidth(float64(scenceWidth))
		p.rect.SetHeight(float64(scenceHeight))
		p.scence.SetSceneRect(p.rect)
	}
}

func (p *Palette) run(text string) {
	p.inputText = text
	p.inputIndex = len(text)
	p.input.Update()
	p.checkFirstC()
	p.viewUpdate()
	p.show()
}

func (p *Palette) paintInput(event *gui.QPaintEvent) {
	painter := gui.NewQPainter2(p.input)
	defer painter.DestroyQPainter()
	padding := p.padding / 2
	color := gui.NewQColor3(p.selectedBg.R, p.selectedBg.G, p.selectedBg.B, p.selectedBg.A)
	painter.FillRect5(
		padding,
		padding,
		1,
		p.inputHeight-2*padding,
		color)
	painter.FillRect5(
		padding+1,
		p.inputHeight-padding-1,
		p.width-2*padding-2,
		1,
		color)
	painter.FillRect5(
		p.width-padding-1,
		padding,
		1,
		p.inputHeight-2*padding,
		color)
	painter.FillRect5(
		padding+1,
		padding,
		p.width-2*padding-2,
		1,
		color)
	painter.SetFont(p.font.font)
	fg := p.editor.theme.Theme.Foreground
	penColor := gui.NewQColor3(fg.R, fg.G, fg.B, fg.A)
	painter.SetPen2(penColor)
	painter.DrawText3(p.padding, padding+int(p.font.shift), p.inputText)

	painter.FillRect5(
		p.padding+int(p.font.fontMetrics.Width(string(p.inputText[:p.inputIndex]))+0.5),
		padding+int(p.font.lineSpace)/2,
		1,
		int(p.font.height+0.5),
		penColor)
}

func (p *Palette) paint(event *gui.QPaintEvent) {
	rect := event.M_rect()

	x := rect.X()
	y := rect.Y()
	width := rect.Width()
	height := rect.Height()

	start := y / int(p.font.lineHeight)
	max := len(p.activeItems) - 1
	painter := gui.NewQPainter2(p.widget)
	defer painter.DestroyQPainter()

	painter.SetFont(p.font.font)

	bg := p.editor.theme.Theme.Background
	painter.FillRect5(x, y, width, height,
		gui.NewQColor3(bg.R, bg.G, bg.B, bg.A))

	for i := start; i < (y+height)/int(p.font.lineHeight)+1; i++ {
		if i > max {
			break
		}
		if p.index == i {
			painter.FillRect5(x, i*int(p.font.lineHeight), width, int(p.font.lineHeight),
				gui.NewQColor3(p.selectedBg.R, p.selectedBg.G, p.selectedBg.B, p.selectedBg.A))
		}
		p.paintLine(painter, i)
	}
}

func (p *Palette) paintLine(painter *gui.QPainter, index int) {
	item := p.activeItems[index]
	y := index*int(p.font.lineHeight) + int(p.font.shift)
	fg := p.editor.theme.Theme.Foreground
	penColor := gui.NewQColor3(fg.R, fg.G, fg.B, fg.A)
	matchedColor := gui.NewQColor3(p.matchFg.R, p.matchFg.G, p.matchFg.B, p.matchFg.A)
	if p.inputType == PaletteLine {
		selection := p.editor.theme.Theme.Selection
		selectionColor := gui.NewQColor3(selection.R, selection.G, selection.B, selection.A)
		lineNumber := fmt.Sprintf("%d ", item.lineNumber)
		painter.SetPen2(selectionColor)
		painter.DrawText3(p.padding, y, lineNumber)
		if item.line != nil {
			padding := int(p.font.fontMetrics.Width(lineNumber)+0.5) + p.padding
			p.editor.activeWin.buffer.drawLine(painter, p.font, item.lineNumber-1, index*int(p.font.lineHeight), padding)
		}
	} else {
		painter.SetPen2(penColor)
		painter.DrawText3(p.padding, y, item.description)
	}

	painter.SetPen2(matchedColor)
	bg := p.editor.theme.Theme.Background
	bgColor := gui.NewQColor3(bg.R, bg.G, bg.B, bg.A)
	selectedBgColor := gui.NewQColor3(p.selectedBg.R, p.selectedBg.G, p.selectedBg.B, p.selectedBg.A)
	for _, match := range item.matches {
		x := p.padding + int(p.font.fontMetrics.Width(strings.Replace(string(item.description[:match]), "\t", p.editor.activeWin.buffer.tabStr, -1))+0.5)
		text := string(item.description[match])
		width := int(p.font.fontMetrics.Width(text) + 0.5)
		painter.FillRect5(x, index*int(p.font.lineHeight), width, int(p.font.lineHeight), bgColor)
		if index == p.index {
			painter.FillRect5(x, index*int(p.font.lineHeight), width, int(p.font.lineHeight), selectedBgColor)
		}
		painter.DrawText3(x, y, string(item.description[match]))
	}

}

func (p *Palette) initCmds() {
	p.cmds = map[string]Command{
		"<Esc>":   p.esc,
		"<C-c>":   p.esc,
		"<Enter>": p.enter,
		"<C-m>":   p.enter,
		"<C-n>":   p.next,
		"<C-p>":   p.previous,
		"<C-u>":   p.deleteToStart,
		"<C-b>":   p.left,
		"<Left>":  p.left,
		"<C-f>":   p.right,
		"<Right>": p.right,
		"<C-h>":   p.deleteLeft,
		"<BS>":    p.deleteLeft,
	}
}

func (p *Palette) executeKey(key string) {
	cmd, ok := p.cmds[key]
	if !ok {
		if strings.HasPrefix(key, "<") && strings.HasSuffix(key, ">") {
			switch key {
			case "<Space>":
				key = " "
			default:
				fmt.Println(key)
				return
			}
		}
		p.inputText = p.inputText[:p.inputIndex] + key + p.inputText[p.inputIndex:]
		p.inputIndex++
		p.input.Update()
		p.checkFirstC()
		p.viewUpdate()
		return
	}
	cmd()
}

func (p *Palette) viewUpdate() {
	p.index = 0
	if p.inputText == "" || p.inputText == string(p.inputType) {
		p.activeItems = p.items
		for _, item := range p.items {
			item.matches = []int{}
		}
	} else {
		p.activeItems = []*PaletteItem{}
		inputText := []rune(p.inputText)
		inputText = inputText[len(p.inputType):]
		for _, item := range p.items {
			score, matches := matchScore([]rune(item.description), inputText)
			if score >= 0 {
				item.score = score
				item.matches = matches
				p.activeItems = append(p.activeItems, item)
			}
		}
		sort.Stable(byScore(p.activeItems))
	}
	p.resize()
	p.widget.Hide()
	p.widget.Show()
	p.goToLine()
	p.scroll()
}

type byScore []*PaletteItem

func (s byScore) Len() int {
	return len(s)
}

func (s byScore) Swap(i, j int) {
	s[i], s[j] = s[j], s[i]
}

func (s byScore) Less(i, j int) bool {
	return s[i].score < s[j].score
}

func (p *Palette) esc() {
	p.resetView()
	p.resetInput()
	p.hide()
}

func (p *Palette) resetView() {
	p.index = 0
	for _, item := range p.items {
		item.matches = []int{}
	}
	switch p.inputType {
	case PaletteLine:
		win := p.editor.activeWin
		win.scrollToCursor(p.oldRow, p.oldCol, true)
	case PaletteThemes:
		p.changeTheme(p.editor.themeName)
	}
}

func (p *Palette) resetInput() {
	p.inputText = ""
	p.inputIndex = 0
	p.inputType = PaletteNone
}

func (p *Palette) enter() {
	item := p.executeItem()
	if !item.stayInPalette {
		p.esc()
	}
}

func (p *Palette) next() {
	p.index++
	if p.index > len(p.activeItems)-1 {
		p.index = 0
	}
	p.widget.Hide()
	p.widget.Show()
	p.switchItem()
	p.scroll()
}

func (p *Palette) switchItem() {
	p.goToLine()
	switch p.inputType {
	case PaletteThemes:
		p.changeTheme(p.activeItems[p.index].description)
	}
}

func (p *Palette) changeTheme(themeName string) {
	p.editor.xi.SetTheme(themeName)
}

func (p *Palette) executeItem() *PaletteItem {
	if p.index >= len(p.activeItems) {
		return nil
	}
	item := p.activeItems[p.index]
	switch p.inputType {
	case PaletteLine:
		p.inputType = PaletteNone
	case PaletteThemes:
		p.editor.changeTheme(item.description)
	case PaletteFile:
		p.editor.openFile(item.description)
	default:
		item.n++

		newIndex := -1
		index := -1
		for i := range p.items {
			if newIndex == -1 && item.n >= p.items[i].n {
				newIndex = i
			}
			if item == p.items[i] {
				index = i
			}
			if newIndex > -1 && index > -1 {
				break
			}
		}
		if newIndex < index {
			copy(p.items[newIndex+1:index+1], p.items[newIndex:index])
			p.items[newIndex] = item
		}
		if item.cmd != nil {
			item.cmd()
		}
	}
	return item
}

func (p *Palette) previous() {
	p.index--
	if p.index < 0 {
		p.index = len(p.activeItems) - 1
	}
	p.widget.Hide()
	p.widget.Show()
	p.switchItem()
	p.scroll()
}

func (p *Palette) scroll() {
	p.view.EnsureVisible2(
		0,
		float64(p.index*int(p.font.lineHeight)),
		1,
		p.font.lineHeight,
		0,
		0,
	)
}

func (p *Palette) deleteToStart() {
	if p.inputIndex == 0 {
		return
	}
	if p.inputType != PaletteFile && p.inputIndex > len(p.inputType) {
		p.inputText = string(p.inputType) + string(p.inputText[p.inputIndex:])
		p.inputIndex = len(p.inputType)
	} else {
		p.inputText = string(p.inputText[p.inputIndex:])
		p.inputIndex = 0
	}
	p.input.Update()
	p.checkFirstC()
	p.viewUpdate()
}

func (p *Palette) left() {
	if p.inputIndex == 0 {
		return
	}
	p.inputIndex--
	p.input.Update()
}

func (p *Palette) right() {
	if p.inputIndex == len(p.inputText) {
		return
	}
	p.inputIndex++
	p.input.Update()
}

func (p *Palette) deleteLeft() {
	if p.inputIndex == 0 {
		return
	}
	p.inputText = string(p.inputText[:p.inputIndex-1]) + string(p.inputText[p.inputIndex:])
	p.inputIndex--
	p.input.Update()
	p.checkFirstC()
	p.viewUpdate()
}

func matchScore(text []rune, pattern []rune) (int, []int) {
	matches := []int{}

	start := 0
	s := 0
	for {
		score, index, n := matchContinuous(text, pattern, start)
		// fmt.Println(string(text), string(pattern), start, score, index, n)
		if score < 0 {
			return -1, nil
		}
		s += score
		for i := 0; i < n; i++ {
			matches = append(matches, index+i)
		}
		if n == len(pattern) {
			return s, matches
		}
		pattern = pattern[n:]
		start = index + n
	}
	return s, matches
}

func matchContinuous(text []rune, pattern []rune, start int) (int, int, int) {
	score := -1
	index := -1
	n := 1
	for {
		newPattern := pattern[:n]
		newScore := -1
		newIndex := -1
		if len(newPattern) == 1 {
			newScore, newIndex = bestMatch(text, start, newPattern[0])
		} else {
			newScore, newIndex = patternIndex(text, newPattern, start)
		}
		if newScore < 0 {
			return score, index, n - 1
		}
		score = newScore
		index = newIndex
		n++
		if n > len(pattern) {
			return score, index, n - 1
		}
	}
}

func patternIndex(text []rune, pattern []rune, start int) (int, int) {
	s := 0
	class := 0
	for i := start; i < len(text); i++ {
		if i == start {
			if patternMatch(text[i:], pattern) {
				return i - start, i
			}
			class = utfClass(text[i])
		} else {
			newClass := utfClass(text[i])
			if newClass != class {
				class = newClass
				s++
				if patternMatch(text[i:], pattern) {
					return i - start, i
				}
			}
		}
	}
	return -1, -1
}

func patternMatch(text []rune, pattern []rune) bool {
	if len(pattern) > len(text) {
		return false
	}
	for i, r := range pattern {
		c := unicode.ToLower(text[i])
		if c != r && text[i] != r {
			return false
		}
	}
	return true
}

func bestMatch(text []rune, start int, r rune) (int, int) {
	class := 0
	s := 0
	for i := start; i < len(text); i++ {
		c := unicode.ToLower(text[i])
		if c == r || text[i] == r {
			if i == start {
				return 0, i
			}
			if utfClass(text[i-1]) != utfClass(r) {
				return s, i
			}
		} else {
			if i == start {
				class = utfClass(text[i])
			} else {
				newClass := utfClass(text[i])
				if newClass != class {
					s++
					class = newClass
				}
			}
		}
	}
	for i := start; i < len(text); i++ {
		c := unicode.ToLower(text[i])
		if c == r || text[i] == r {
			return i * 10, i
		}
	}
	return -1, -1
}

func (p *Palette) checkFirstC() {
	firstC := p.getInputType()
	if firstC == p.inputType {
		return
	}
	p.resetView()
	p.inputType = firstC
	switch firstC {
	case PaletteCommand:
		p.items = p.editor.allCmds()
	case PaletteLine:
		win := p.editor.activeWin
		p.oldRow = win.row
		p.oldCol = win.col
		p.items = p.editor.getCurrentBufferLinePaletteItems()
	case PaletteFile:
		p.items = p.editor.getFilePaletteItems()
	case PaletteThemes:
		p.items = p.editor.allThemes()
	default:
		p.items = []*PaletteItem{}
	}
	p.activeItems = p.items
	p.resize()
}

func (p *Palette) goToLine() {
	if p.inputType != PaletteLine {
		return
	}
	if p.inputText == "#" {
		return
	}
	if len(p.activeItems) == 0 {
		return
	}

	item := p.activeItems[p.index]
	win := p.editor.activeWin
	win.scrollToCursor(item.lineNumber-1, 0, true)
}

func (p *Palette) getInputType() string {
	if p.inputText == "" {
		return PaletteFile
	}

	if strings.HasPrefix(p.inputText, ":themes") {
		return PaletteThemes
	}

	switch string(p.inputText[0]) {
	case PaletteCommand:
		return PaletteCommand
	case PaletteLine:
		return PaletteLine
	default:
	}
	return PaletteFile
}

func (p *Palette) show() {
	if p.active {
		return
	}
	p.active = true
	p.mainWidget.Show()
	p.view.VerticalScrollBar().SetValue(0)
}

func (p *Palette) hide() {
	if !p.active {
		return
	}
	p.active = false
	p.mainWidget.Hide()
}
