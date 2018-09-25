package editor

import (
	"strings"

	"github.com/dzhou121/crane/lsp"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

type popupSignal struct {
	core.QObject
	_ func() `signal:"updateSignal"`
}

// Popup is the popup menu for auto complete
type Popup struct {
	editor       *Editor
	signal       *popupSignal
	view         *widgets.QGraphicsView
	scence       *widgets.QGraphicsScene
	widget       *widgets.QWidget
	rect         *core.QRectF
	font         *Font
	updates      chan interface{}
	shown        bool
	items        []*lsp.CompletionItem
	cmds         map[string]Command
	index        int
	width        int
	height       int
	x            int
	y            int
	scenceHeight int
}

func newPopup(editor *Editor) *Popup {
	p := &Popup{
		editor:  editor,
		scence:  widgets.NewQGraphicsScene(nil),
		view:    widgets.NewQGraphicsView(nil),
		widget:  widgets.NewQWidget(nil, 0),
		rect:    core.NewQRectF(),
		font:    editor.monoFont,
		signal:  NewPopupSignal(nil),
		updates: make(chan interface{}, 1000),
	}
	p.initCmds()
	p.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)
	p.view.SetHorizontalScrollBarPolicy(core.Qt__ScrollBarAlwaysOff)
	p.view.SetFrameStyle(0)
	p.scence.AddWidget(p.widget, 0).SetPos2(0, 0)
	p.view.SetScene(p.scence)
	p.widget.ConnectPaintEvent(p.paint)
	shadow := widgets.NewQGraphicsDropShadowEffect(nil)
	shadow.SetBlurRadius(20)
	shadow.SetColor(gui.NewQColor3(0, 0, 0, 255))
	shadow.SetOffset3(0, 2)
	p.view.SetGraphicsEffect(shadow)
	p.signal.ConnectUpdateSignal(func() {
		update := <-p.updates
		switch u := update.(type) {
		case []*lsp.CompletionItem:
			p.showItems(u)
		case *lsp.Position:
			p.move(u)
		}
	})
	p.view.Hide()
	return p
}

func (p *Popup) paint(event *gui.QPaintEvent) {
	rect := event.M_rect()
	x := rect.X()
	y := rect.Y()
	width := rect.Width()
	height := rect.Height()

	start := y / int(p.font.lineHeight)
	max := len(p.items) - 1
	painter := gui.NewQPainter2(p.widget)
	defer painter.DestroyQPainter()

	painter.SetFont(p.font.font)
	bg := p.editor.theme.Theme.Background
	selectedBg := p.editor.selectedBg
	painter.FillRect5(x, y, width, height,
		gui.NewQColor3(bg.R, bg.G, bg.B, bg.A))
	for i := start; i < (y+height)/int(p.font.lineHeight)+1; i++ {
		if i > max {
			break
		}
		if p.index == i {
			painter.FillRect5(int(p.font.lineHeight), i*int(p.font.lineHeight), width, int(p.font.lineHeight),
				gui.NewQColor3(selectedBg.R, selectedBg.G, selectedBg.B, selectedBg.A))
		}
		p.paintLine(painter, i)
	}
}

func (p *Popup) paintLine(painter *gui.QPainter, index int) {
	item := p.items[index]
	y := index*int(p.font.lineHeight) + int(p.font.shift)
	font := p.editor.activeWin.buffer.font

	color := newColor(151, 195, 120, 255)
	bg := newColor(151, 195, 120, 51)
	kindText := ""

	switch item.Kind {
	case lsp.Function:
		kindText = "f"
		color = newColor(97, 174, 239, 255)
		bg = newColor(97, 174, 239, 51)
	case lsp.Variable:
		kindText = "v"
		color = newColor(223, 106, 115, 255)
		bg = newColor(223, 106, 115, 51)
	case lsp.Constant:
		kindText = "c"
		color = newColor(223, 106, 115, 255)
		bg = newColor(223, 106, 115, 51)
	case lsp.Class:
		kindText = "c"
		color = newColor(229, 193, 124, 255)
		bg = newColor(229, 193, 124, 50)
	case lsp.Method:
		kindText = "t"
		color = newColor(229, 193, 124, 255)
		bg = newColor(229, 193, 124, 50)
	case lsp.Module:
		kindText = "m"
		color = newColor(42, 161, 152, 255)
		bg = newColor(42, 161, 152, 51)
	case lsp.Keyword:
		kindText = "k"
		color = newColor(42, 161, 152, 255)
		bg = newColor(42, 161, 152, 51)
	case lsp.Reference:
		kindText = "p"
		color = newColor(42, 161, 152, 255)
		bg = newColor(42, 161, 152, 50)
	default:
		kindText = ""
	}
	lineHeight := int(font.lineHeight)
	padding := int(p.font.width)
	typeColor := gui.NewQColor3(color.R, color.G, color.B, color.A)
	painter.SetPen2(typeColor)
	painter.FillRect5(0, index*int(p.font.lineHeight), lineHeight, lineHeight,
		gui.NewQColor3(bg.R, bg.G, bg.B, bg.A))
	painter.DrawText3(int((font.lineHeight-font.width)/2), y, kindText)

	fg := p.editor.theme.Theme.Foreground
	penColor := gui.NewQColor3(fg.R, fg.G, fg.B, fg.A)
	painter.SetPen2(penColor)
	painter.DrawText3(lineHeight+padding, y, item.InsertText+" "+item.Detail)

	matchFg := p.editor.matchFg
	matchedColor := gui.NewQColor3(matchFg.R, matchFg.G, matchFg.B, matchFg.A)
	painter.SetPen2(matchedColor)
	newBg := p.editor.theme.Theme.Background
	bgColor := gui.NewQColor3(newBg.R, newBg.G, newBg.B, newBg.A)
	selectedBg := p.editor.selectedBg
	selectedBgColor := gui.NewQColor3(selectedBg.R, selectedBg.G, selectedBg.B, selectedBg.A)
	for _, match := range item.Matches {
		x := lineHeight + padding + int(p.font.fontMetrics.Size(0, strings.Replace(string(item.InsertText[:match]), "\t", p.editor.activeWin.buffer.tabStr, -1), 0, 0).Rwidth()+0.5)
		text := string(item.InsertText[match])
		width := int(p.font.fontMetrics.Size(0, text, 0, 0).Rwidth() + 0.5)
		painter.FillRect5(x, index*int(p.font.lineHeight), width, int(p.font.lineHeight), bgColor)
		if index == p.index {
			painter.FillRect5(x, index*int(p.font.lineHeight), width, int(p.font.lineHeight), selectedBgColor)
		}
		painter.DrawText3(x, y, string(item.InsertText[match]))
	}
}

func (p *Popup) show() {
	if p.shown {
		return
	}
	p.shown = true
	p.view.Show()
	p.view.VerticalScrollBar().SetValue(0)
}

func (p *Popup) hide() {
	if !p.shown {
		return
	}
	p.shown = false
	p.index = 0
	p.view.Hide()
	if len(p.items) > 0 {
		p.editor.lspClient.resetCompletion(p.editor.activeWin.buffer)
	}
}

func (p *Popup) updatePos(pos *lsp.Position) {
	p.updates <- pos
	p.signal.UpdateSignal()
}

func (p *Popup) updateItems(items []*lsp.CompletionItem) {
	p.updates <- items
	p.signal.UpdateSignal()
}

func (p *Popup) showItems(items []*lsp.CompletionItem) {
	if len(items) == 0 {
		p.hide()
		p.items = items
		return
	}
	p.items = items
	p.index = 0
	p.resize()
	p.show()
	p.widget.Update()
}

func (p *Popup) move(pos *lsp.Position) {
	row := pos.Line
	col := pos.Character
	win := p.editor.activeWin
	x, y := win.buffer.getPos(row, col)
	x = x - win.horizontalScrollValue - int(p.font.lineHeight) - int(p.font.width)
	y = y - win.verticalScrollValue
	y += int(win.buffer.font.lineHeight)
	if x != p.x || y != p.y {
		p.x = x
		p.y = y
		p.view.Move2(x, y)
	}
}

func (p *Popup) resize() {
	// win := p.editor.activeWin
	// x := win.x
	// y := win.y + int(win.buffer.font.lineHeight)
	// if x != p.x || y != p.y {
	// 	p.x = x
	// 	p.y = y
	// 	p.view.Move2(x, y)
	// }
	maxHeight := int(float64(p.editor.height)*0.382 + 0.5)
	num := maxHeight / int(p.font.lineHeight)
	if len(p.items) < num {
		num = len(p.items)
	}
	p.width = 400
	height := num * int(p.font.lineHeight)
	if p.height != height {
		p.height = height
		p.view.Resize2(p.width, height)
	}

	scenceHeight := len(p.items) * int(p.font.lineHeight)
	if p.scenceHeight != scenceHeight {
		p.scenceHeight = scenceHeight
		scenceWidth := p.width
		if scenceHeight > height {
			scenceWidth -= 16
		}
		p.widget.Resize2(scenceWidth, scenceHeight)
		p.rect.SetWidth(float64(scenceWidth))
		p.rect.SetHeight(float64(scenceHeight))
		p.scence.SetSceneRect(p.rect)
	}
}

func (p *Popup) executeKey(key string) bool {
	cmd, ok := p.cmds[key]
	if !ok {
		return false
	}
	cmd()
	return true
}

func (p *Popup) initCmds() {
	p.cmds = map[string]Command{
		"<C-n>":   p.next,
		"<C-p>":   p.previous,
		"<Tab>":   p.selectItem,
		"<C-m>":   p.selectItem,
		"<Enter>": p.selectItem,
	}
}

func (p *Popup) selectItem() {
	item := p.items[p.index]
	p.editor.lspClient.selectCompletionItem(p.editor.activeWin.buffer, item)
}

func (p *Popup) next() {
	p.index++
	if p.index > len(p.items)-1 {
		p.index = 0
	}
	p.widget.Update()
	p.scroll()
}

func (p *Popup) previous() {
	p.index--
	if p.index < 0 {
		p.index = len(p.items) - 1
	}
	p.widget.Update()
	p.scroll()
}

func (p *Popup) scroll() {
	p.view.EnsureVisible2(
		0,
		float64(p.index*int(p.font.lineHeight)),
		1,
		p.font.lineHeight,
		0,
		0,
	)
}
