package editor

import "github.com/therecipe/qt/widgets"

// DiagPopup is
type DiagPopup struct {
	win          *Window
	widget       *widgets.QWidget
	contentLabel *widgets.QLabel
	contentText  string
	shown        bool
}

func newDiagPopup(win *Window) *DiagPopup {
	p := &DiagPopup{
		win:    win,
		widget: widgets.NewQWidget(nil, 0),
	}
	p.widget.SetParent(win.view)

	layout := widgets.NewQHBoxLayout()
	layout.SetContentsMargins(0, 0, 0, 0)
	layout.SetSpacing(4)
	p.widget.SetContentsMargins(8, 8, 8, 8)
	p.widget.SetLayout(layout)
	p.widget.SetStyleSheet(".QWidget { border: 1px solid #000; } * {color: rgba(205, 211, 222, 1); background-color: rgba(24, 29, 34, 1);}")

	p.contentLabel = widgets.NewQLabel(nil, 0)
	p.contentLabel.SetContentsMargins(0, 0, 0, 0)

	layout.AddWidget(p.contentLabel, 0, 0)

	p.widget.Hide()

	return p
}

func (p *DiagPopup) hide() {
	if !p.shown {
		return
	}
	p.shown = false
	p.widget.Hide()
}

func (p *DiagPopup) show() {
	if p.shown {
		return
	}
	p.shown = true
	p.widget.Show()
}
