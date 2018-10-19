package editor

import (
	"fmt"
	"path/filepath"
	"sort"
	"strings"

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

type byLine []*lsp.Diagnostics

func (s byLine) Len() int {
	return len(s)
}

func (s byLine) Swap(i, j int) {
	s[i], s[j] = s[j], s[i]
}

func (s byLine) Less(i, j int) bool {
	return s[i].Range.Start.Line < s[j].Range.Start.Line
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

func (d *DiagnosticsPanel) changeSize(count int) {
}

func (d *DiagnosticsPanel) update() {
	width := 0
	n := 0
	d.diagnostics = []*lsp.PublishDiagnosticsParams{}
	for _, params := range d.editor.diagnostics {
		if len(params.Diagnostics) == 0 {
			continue
		}
		d.diagnostics = append(d.diagnostics, params)
		n++
		for _, diagnostic := range params.Diagnostics {
			n++
			w := int(d.font.fontMetrics.Size(0, diagnostic.Message, 0, 0).Rwidth() + 1)
			if w > width {
				width = w
			}
		}
	}
	for _, param := range d.diagnostics {
		for _, diag := range param.Diagnostics {
			fmt.Println(diag.Message)
		}
	}
	height := int(d.font.lineHeight * float64(n+1))
	sort.Sort(byURI(d.diagnostics))
	log.Infoln(d.diagnostics)
	width = 800

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
		d.paintFile(painter, params.URI, i)
	innerLoop:
		for _, diagnostics := range params.Diagnostics {
			i++
			if i < start {
				continue innerLoop
			}
			if i >= end {
				break loop
			}
			d.paintDiagnostic(painter, diagnostics, i)
		}
	}
}

func (d *DiagnosticsPanel) paintFile(painter *gui.QPainter, file string, index int) {
	padding := 10
	y := index*int(d.font.lineHeight) + int(d.font.shift)
	r := core.NewQRectF()
	r.SetX(float64(padding))
	r.SetY(float64(index)*d.font.lineHeight + (d.font.lineSpace / 2))
	r.SetWidth(d.font.height)
	r.SetHeight(d.font.height)

	penColor := gui.NewQColor3(205, 211, 222, 255)
	painter.SetPen2(penColor)

	file = string(file[7:])
	file = strings.Replace(file, d.editor.cwd+"/", "", 1)
	base := filepath.Base(file)
	dir := filepath.Dir(file)
	if dir == "." {
		dir = ""
	}
	fileType := filepath.Ext(file)
	if fileType != "" {
		fileType = string(fileType[1:])
	}
	if fileType == "" {
		fileType = "default"
	}
	svg := d.editor.getSvgRenderer(fileType, nil)
	svg.Render2(painter, r)

	padding += int(d.font.height + 5)
	painter.SetPen2(penColor)
	painter.DrawText3(padding, y, base)

	if dir != "" {
		padding += 5
		penColor = gui.NewQColor3(131, 131, 131, 255)
		painter.SetPen2(penColor)
		painter.DrawText3(padding+int(d.font.fontMetrics.Size(0, base, 0, 0).Rwidth()), y, dir)
	}
}

func (d *DiagnosticsPanel) paintDiagnostic(painter *gui.QPainter, diag *lsp.Diagnostics, index int) {
	y := index*int(d.font.lineHeight) + int(d.font.shift)
	padding := int(15 + d.font.height)

	r := core.NewQRectF()
	r.SetX(float64(padding))
	r.SetY(float64(index)*d.font.lineHeight + (d.font.lineSpace / 2))
	r.SetWidth(d.font.height)
	r.SetHeight(d.font.height)

	icon := "times-circle"
	if diag.Severity == 2 {
		icon = "exclamation-triangle"
	} else if diag.Severity == 1 {
		icon = "times-circle"
	}
	svg := d.editor.getSvgRenderer(icon, nil)
	svg.Render2(painter, r)

	padding += int(d.font.height + 5)

	penColor := gui.NewQColor3(205, 211, 222, 255)
	painter.SetPen2(penColor)
	painter.DrawText3(padding, y, diag.Message)

	penColor = gui.NewQColor3(131, 131, 131, 255)
	painter.SetPen2(penColor)
	painter.DrawText3(int(float64(padding)+5+d.font.fontMetrics.Size(0, diag.Message, 0, 0).Rwidth()),
		y,
		fmt.Sprintf("(%d, %d)", diag.Range.Start.Line+1, diag.Range.End.Character+1),
	)
}

func (d *DiagnosticsPanel) paintLine(painter *gui.QPainter, padding int, text string, index int) {
	y := index*int(d.font.lineHeight) + int(d.font.shift)
	fg := d.editor.theme.Theme.Foreground
	penColor := gui.NewQColor3(fg.R, fg.G, fg.B, fg.A)
	painter.SetPen2(penColor)
	painter.DrawText3(padding, y, text)
}
