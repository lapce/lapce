package editor

import (
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/dzhou121/crane/fuzzy"
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

type paletteSignal struct {
	core.QObject
	_ func() `signal:"updateSignal"`
}

// Palette is
type Palette struct {
	editor               *Editor
	signal               *paletteSignal
	mainWidget           *widgets.QWidget
	input                *widgets.QWidget
	view                 *widgets.QGraphicsView
	scence               *widgets.QGraphicsScene
	widget               *widgets.QWidget
	rect                 *core.QRectF
	font                 *Font
	active               bool
	running              bool
	itemsRWMutex         sync.RWMutex
	activeItemsRWMutex   sync.RWMutex
	itemsChan            chan *PaletteItem
	items                []*PaletteItem
	activeItems          []*PaletteItem
	shownItems           []*PaletteItem
	index                int
	cmds                 map[string]Command
	cancelGetChan        chan struct{}
	cancelLastChan       chan struct{}
	inputMutex           sync.Mutex
	inputText            string
	inputIndex           int
	paintAfterViewUpdate bool

	inputType string

	oldRow           int
	oldCol           int
	oldVerticalValue int

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
		signal:     NewPaletteSignal(nil),
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
	p.signal.ConnectUpdateSignal(func() {
		p.resize()
		if !p.checkPaintItems() {
			p.widget.Hide()
			p.widget.Show()
		}
	})
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
	total := 0
	if len(p.inputText) > len(p.inputType) {
		p.activeItemsRWMutex.RLock()
		total = len(p.activeItems)
		p.activeItemsRWMutex.RUnlock()
	} else {
		p.itemsRWMutex.RLock()
		total = len(p.items)
		p.itemsRWMutex.RUnlock()
	}
	n := total
	if n > max {
		n = max
	}
	viewHeight := n * int(p.font.lineHeight)
	if viewHeight != p.viewHeight {
		p.viewHeight = viewHeight
		p.view.SetFixedSize2(p.width, viewHeight)
	}
	scenceHeight := total * int(p.font.lineHeight)
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
	p.show()
}

func (p *Palette) run(text string) {
	p.inputText = text
	p.inputIndex = len(text)
	p.input.Update()
	p.checkInputType()
	p.viewUpdate()
	p.running = true
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
		p.padding+int(p.font.fontMetrics.Size(0, string(p.inputText[:p.inputIndex]), 0, 0).Rwidth()+0.5),
		padding+int(p.font.lineSpace)/2,
		1,
		int(p.font.height+0.5),
		penColor)
}

func (p *Palette) checkPaintItems() bool {
	if !p.paintAfterViewUpdate {
		p.paintAfterViewUpdate = true
		return false
	}
	var items []*PaletteItem
	if len(p.inputText) > len(p.inputType) {
		items = p.activeItems
	} else {
		items = p.items
	}

	y := p.view.VerticalScrollBar().Value()
	start := y / int(p.font.lineHeight)
	num := p.viewHeight/int(p.font.lineHeight) + 1
	end := start + num
	if end > len(items) {
		end = len(items)
		num = end - start
	}
	if len(p.shownItems) < num {
		for i := 0; i < num-len(p.shownItems); i++ {
			p.shownItems = append(p.shownItems, nil)
		}
	} else if len(p.shownItems) > num {
		p.shownItems = p.shownItems[:num]
	} else {
		same := true
		for i := range p.shownItems {
			if p.shownItems[i] != items[i] {
				same = false
				p.shownItems[i] = items[i]
			}
		}
		return same
	}
	copy(p.shownItems, items[start:end])
	return false
}

func matchesSame(old []int, new []int) bool {
	if len(old) != len(new) {
		return false
	}
	for i := range old {
		if old[i] != new[i] {
			return false
		}
	}
	return true
}

func (p *Palette) paint(event *gui.QPaintEvent) {
	var items []*PaletteItem
	if len(p.inputText) > len(p.inputType) {
		items = p.activeItems
	} else {
		items = p.items
	}
	rect := event.M_rect()

	x := rect.X()
	y := rect.Y()
	width := rect.Width()
	height := rect.Height()

	start := y / int(p.font.lineHeight)
	max := len(items) - 1
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
	var items []*PaletteItem
	if len(p.inputText) > len(p.inputType) {
		items = p.activeItems
	} else {
		items = p.items
		items[index].matches = []int{}
	}
	item := items[index]
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
			padding := int(p.font.fontMetrics.Size(0, lineNumber, 0, 0).Rwidth()+0.5) + p.padding
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
		x := p.padding + int(p.font.fontMetrics.Size(0, strings.Replace(string(item.description[:match]), "\t", p.editor.activeWin.buffer.tabStr, -1), 0, 0).Rwidth()+0.5)
		text := string(item.description[match])
		width := int(p.font.fontMetrics.Size(0, text, 0, 0).Rwidth() + 0.5)
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
		p.checkInputType()
		p.viewUpdate()
		return
	}
	cmd()
}

func (p *Palette) viewUpdate() {
	p.index = 0
	p.view.VerticalScrollBar().SetValue(0)
	p.updateActiveItems()
	return
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
	p.running = false
	p.hide()
}

func (p *Palette) resetView() {
	if p.cancelGetChan != nil {
		close(p.cancelGetChan)
		p.cancelGetChan = nil
	}
	if p.cancelLastChan != nil {
		close(p.cancelLastChan)
		p.cancelLastChan = nil
	}
	p.index = 0
	p.view.VerticalScrollBar().SetValue(0)
	for _, item := range p.items {
		item.matches = []int{}
	}
	switch p.inputType {
	case PaletteLine:
		win := p.editor.activeWin
		win.verticalScrollBar.SetValue(p.oldVerticalValue)
		win.scrollToCursor(p.oldRow, p.oldCol, true, false, false)
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
	var items []*PaletteItem
	if len(p.inputText) > len(p.inputType) {
		items = p.activeItems
	} else {
		items = p.items
	}

	p.index++
	if p.index > len(items)-1 {
		p.index = 0
	}
	p.widget.Hide()
	p.widget.Show()
	p.switchItem()
	p.scroll()
}

func (p *Palette) switchItem() {
	switch p.inputType {
	case PaletteThemes:
		var items []*PaletteItem
		if len(p.inputText) > len(p.inputType) {
			items = p.activeItems
		} else {
			items = p.items
		}
		p.changeTheme(items[p.index].description)
	case PaletteLine:
		var items []*PaletteItem
		if len(p.inputText) > len(p.inputType) {
			items = p.activeItems
		} else {
			items = p.items
		}
		if len(items) == 0 {
			return
		}
		if p.index > len(items) {
			p.index = 0
		}
		item := items[p.index]
		win := p.editor.activeWin
		row := item.lineNumber - 1
		col := 0
		win.verticalScrollBar.SetValue(row*int(win.buffer.font.lineHeight) - win.frame.height*2/3)
		win.setPos(row, col, false)
	}
}

func (p *Palette) changeTheme(themeName string) {
	p.editor.xi.SetTheme(themeName)
}

func (p *Palette) executeItem() *PaletteItem {
	var items []*PaletteItem
	if len(p.inputText) > len(p.inputType) {
		items = p.activeItems
	} else {
		items = p.items
	}
	if p.index >= len(items) {
		return nil
	}
	item := items[p.index]
	switch p.inputType {
	case PaletteLine:
		p.inputType = PaletteNone

		win := p.editor.activeWin
		row := item.lineNumber - 1
		col := 0
		win.verticalScrollBar.SetValue(row*int(win.buffer.font.lineHeight) - win.frame.height*2/3)
		win.setPos(row, col, false)
	case PaletteThemes:
		p.editor.changeTheme(item.description)
	case PaletteFile:
		path := filepath.Join(p.editor.cwd, item.description)
		p.editor.activeWin.openFile(path)
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
	var items []*PaletteItem
	if len(p.inputText) > len(p.inputType) {
		items = p.activeItems
	} else {
		items = p.items
	}

	p.index--
	if p.index < 0 {
		p.index = len(items) - 1
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
	p.checkInputType()
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
	p.checkInputType()
	p.viewUpdate()
}

func (p *Palette) checkInputType() {
	inputType := p.getInputType()
	if inputType == p.inputType {
		return
	}
	p.resetView()
	p.inputType = inputType
	p.getItems(inputType)
	// switch firstC {
	// case PaletteCommand:
	// 	p.items = p.editor.allCmds()
	// case PaletteLine:
	// 	win := p.editor.activeWin
	// 	p.oldRow = win.row
	// 	p.oldCol = win.col
	// 	p.items = p.editor.getCurrentBufferLinePaletteItems()
	// case PaletteFile:
	// 	p.items = p.editor.getFilePaletteItems()
	// case PaletteThemes:
	// 	p.items = p.editor.allThemes()
	// default:
	// 	p.items = []*PaletteItem{}
	// }
	// p.activeItems = p.items
	// p.resize()
}

func (p *Palette) updateActiveItem(item *PaletteItem) {
	if len(p.inputText) <= len(p.inputType) {
		return
	}
	inputText := []rune(p.inputText[len(p.inputType):])
	score, matches := fuzzy.MatchScore([]rune(item.description), inputText)
	if score > -1 {
		i := 0
		p.activeItemsRWMutex.Lock()
		for i = 0; i < len(p.activeItems); i++ {
			activeItem := p.activeItems[i]
			if score < activeItem.score {
				break
			}
		}
		item.score = score
		item.matches = matches
		p.activeItems = append(p.activeItems, nil)
		copy(p.activeItems[i+1:], p.activeItems[i:])
		p.activeItems[i] = item
		p.activeItemsRWMutex.Unlock()
	}
}

func (p *Palette) updateActiveItems() {
	if p.cancelLastChan != nil {
		close(p.cancelLastChan)
		p.cancelLastChan = nil
	}
	// if len(p.inputText) <= len(p.inputType) {
	// 	return
	// }
	p.activeItemsRWMutex.Lock()
	p.activeItems = []*PaletteItem{}
	cancelLastChan := make(chan struct{})
	p.cancelLastChan = cancelLastChan
	p.activeItemsRWMutex.Unlock()

	p.paintAfterViewUpdate = false

	go func() {
		ticker := time.NewTicker(20 * time.Millisecond)
		defer func() {
			p.signal.UpdateSignal()
			ticker.Stop()
		}()

		itemsChan := newInfiniteChannel()
		input := itemsChan.In()
		output := itemsChan.Out()
		length := len(p.items)
		go func() {
			for {
				select {
				case <-cancelLastChan:
					return
				case item, ok := <-p.itemsChan:
					if !ok {
						itemsChan.close()
						return
					}
					p.items = append(p.items, item)
					select {
					case input <- item:
					case <-cancelLastChan:
						return
					}
				}
			}
		}()

		for i := 0; i < length; {
			select {
			case <-ticker.C:
				p.signal.UpdateSignal()
			case <-cancelLastChan:
				return
			default:
				item := p.items[i]
				p.updateActiveItem(item)
				i++
			}
		}

		for {
			select {
			case <-ticker.C:
				p.signal.UpdateSignal()
			case <-cancelLastChan:
				return
			case item, ok := <-output:
				if !ok {
					return
				}
				p.updateActiveItem(item)
			}
		}
	}()
}

func (p *Palette) getItems(inputType string) {
	if p.cancelGetChan != nil {
		close(p.cancelGetChan)
		p.cancelGetChan = nil
	}

	p.itemsRWMutex.Lock()
	p.items = []*PaletteItem{}
	p.shownItems = []*PaletteItem{}
	cancelGetChan := make(chan struct{})
	p.cancelGetChan = cancelGetChan
	p.itemsRWMutex.Unlock()

	var itemsChan chan *PaletteItem
	switch inputType {
	case PaletteCommand:
		p.items = p.editor.allCmds()
		itemsChan := make(chan *PaletteItem)
		close(itemsChan)
	case PaletteFile:
		itemsChan = p.editor.getFilePaletteItemsChan()
	case PaletteLine:
		win := p.editor.activeWin
		p.oldRow = win.row
		p.oldCol = win.col
		p.oldVerticalValue = win.verticalScrollBar.Value()
		itemsChan = p.editor.getCurrentBufferLinePaletteItemsChan()
	case PaletteThemes:
		p.items = p.editor.allThemes()
		itemsChan := make(chan *PaletteItem)
		close(itemsChan)
	default:
	}
	p.itemsChan = itemsChan
	if itemsChan == nil {
		return
	}
	return
	go func() {
		ticker := time.NewTicker(20 * time.Millisecond)
		defer func() {
			p.signal.UpdateSignal()
			ticker.Stop()
		}()
		for {
			select {
			case <-ticker.C:
				p.signal.UpdateSignal()
			case <-cancelGetChan:
				return
			case item, ok := <-itemsChan:
				if !ok {
					return
				}
				p.itemsRWMutex.Lock()
				p.items = append(p.items, item)
				// inputText := p.inputText[len(p.inputType):]
				// if inputText == "" {
				// 	p.activeItems = append(p.activeItems, item)
				// } else {
				// 	score, matches := matchScore([]rune(item.description), []rune(inputText))
				// 	if score > -1 {
				// 		i := 0
				// 		var activeItem *PaletteItem
				// 		for i, activeItem = range p.activeItems {
				// 			if score > activeItem.score {
				// 				break
				// 			}
				// 		}
				// 		item.score = score
				// 		item.matches = matches
				// 		p.activeItems = append(p.activeItems, nil)
				// 		copy(p.activeItems[i:], p.activeItems[i+1:])
				// 		p.activeItems[i] = item
				// 	}
				// }
				// if len(p.activeItems) == 20 {
				// 	p.signal.UpdateSignal()
				// }
				p.itemsRWMutex.Unlock()
			}
		}
	}()
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
	if !p.running {
		return
	}
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

// InfiniteChannel implements the Channel interface with an infinite buffer between the input and the output.
type InfiniteChannel struct {
	input, output chan *PaletteItem
	length        chan int
	buffer        *Queue
}

func newInfiniteChannel() *InfiniteChannel {
	ch := &InfiniteChannel{
		input:  make(chan *PaletteItem),
		output: make(chan *PaletteItem),
		length: make(chan int),
		buffer: NewQueue(),
	}
	go ch.infiniteBuffer()
	return ch
}

// In of
func (ch *InfiniteChannel) In() chan<- *PaletteItem {
	return ch.input
}

// Out of
func (ch *InfiniteChannel) Out() <-chan *PaletteItem {
	return ch.output
}

// Len of
func (ch *InfiniteChannel) Len() int {
	return <-ch.length
}

// func (ch *InfiniteChannel) Cap() BufferCap {
// 	return Infinity
// }

func (ch *InfiniteChannel) close() {
	close(ch.input)
}

func (ch *InfiniteChannel) infiniteBuffer() {
	var input, output chan *PaletteItem
	var next *PaletteItem
	input = ch.input

	for input != nil || output != nil {
		select {
		case elem, open := <-input:
			if open {
				ch.buffer.Add(elem)
			} else {
				input = nil
			}
		case output <- next:
			ch.buffer.Remove()
		case ch.length <- ch.buffer.Length():
		}

		if ch.buffer.Length() > 0 {
			output = ch.output
			next = ch.buffer.Peek()
		} else {
			output = nil
			next = nil
		}
	}

	close(ch.output)
	close(ch.length)
}
