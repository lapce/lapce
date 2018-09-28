package editor

import (
	"fmt"
	"sort"

	"github.com/crane-editor/crane/log"
	"github.com/crane-editor/crane/lsp"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

type byURI []*lsp.PublishDiagnosticsParams

func (s byURI) Len() int {
	return len(s)
}

func (s byURI) Swap(i, j int) {
	s[i], s[j] = s[j], s[i]
}

func (s byURI) Less(i, j int) bool {
	return s[i].URI < s[j].URI
}

// DiagnosticsPanel is
type DiagnosticsPanel struct {
	editor      *Editor
	font        *Font
	widget      *widgets.QWidget
	scence      *widgets.QGraphicsScene
	view        *widgets.QGraphicsView
	width       int
	height      int
	rect        *core.QRectF
	diagnostics []*lsp.PublishDiagnosticsParams
}

func newDiagnositicsPanel(editor *Editor) *DiagnosticsPanel {
	d := &DiagnosticsPanel{
		editor: editor,
		widget: widgets.NewQWidget(nil, 0),
		font:   editor.defaultFont,
		scence: widgets.NewQGraphicsScene(nil),
		view:   widgets.NewQGraphicsView(nil),
		rect:   core.NewQRectF(),
	}
	d.scence.SetBackgroundBrush(editor.bgBrush)
	d.scence.AddWidget(d.widget, 0).SetPos2(0, 0)
	d.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)
	d.view.SetFrameStyle(0)
	d.view.SetScene(d.scence)
	d.widget.SetFixedSize2(0, 0)
	d.widget.ConnectPaintEvent(d.paint)
	d.rect.SetWidth(1)
	d.rect.SetHeight(1)
	d.scence.SetSceneRect(d.rect)

	return d
}

func (d *DiagnosticsPanel) update() {
	width := 0
	n := 0
	d.diagnostics = []*lsp.PublishDiagnosticsParams{}
	for _, params := range d.editor.diagnostics {
		d.diagnostics = append(d.diagnostics, params)
		for _, diagnostic := range params.Diagnostics {
			n++
			w := int(d.font.fontMetrics.Size(0, diagnostic.Message, 0, 0).Rwidth() + 1)
			if w > width {
				width = w
			}
		}
	}
	height := int(d.font.lineHeight*float64(n) + 1)
	sort.Sort(byURI(d.diagnostics))
	log.Infoln(d.diagnostics)

	if width != d.width || height != d.height {
		d.width = width
		d.height = height
		d.widget.SetFixedSize2(d.width, d.height)
		d.rect.SetWidth(float64(d.width + 1))
		d.rect.SetHeight(float64(d.height + 1))
		d.scence.SetSceneRect(d.rect)
	}

	d.widget.Update()
}

func (d *DiagnosticsPanel) paint(event *gui.QPaintEvent) {
	rect := event.M_rect()
	x := rect.X()
	y := rect.Y()
	width := rect.Width()
	height := rect.Height()

	start := y / int(d.font.lineHeight)
	end := (y+height)/int(d.font.lineHeight) + 1
	// max := len(d.diagnostics) - 1

	painter := gui.NewQPainter2(d.widget)
	defer painter.DestroyQPainter()

	painter.SetFont(d.font.font)

	bg := d.editor.theme.Theme.Background
	painter.FillRect5(x, y, width, height,
		gui.NewQColor3(bg.R, bg.G, bg.B, bg.A))

	i := -1
loop:
	for _, params := range d.diagnostics {
		i++
		d.paintLine(painter, 0, params.URI, i)
	innerLoop:
		for _, diagnostics := range params.Diagnostics {
			i++
			if i < start {
				continue innerLoop
			}
			if i >= end {
				break loop
			}
			d.paintLine(painter, 20, fmt.Sprintf("%s (%d %d)", diagnostics.Message, diagnostics.Range.Start.Line, diagnostics.Range.Start.Character), i)
		}
	}
}

func (d *DiagnosticsPanel) paintLine(painter *gui.QPainter, padding int, text string, index int) {
	y := index*int(d.font.lineHeight) + int(d.font.shift)
	fg := d.editor.theme.Theme.Foreground
	penColor := gui.NewQColor3(fg.R, fg.G, fg.B, fg.A)
	painter.SetPen2(penColor)
	painter.DrawText3(padding, y, text)
}
