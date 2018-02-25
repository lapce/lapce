package editor

import (
	"github.com/therecipe/qt/gui"
)

// Font is
type Font struct {
	font         *gui.QFont
	fontMetrics  *gui.QFontMetricsF
	width        float64
	height       float64
	ascent       float64
	descent      float64
	leading      float64
	shift        float64
	lineHeight   float64
	underlinePos float64
	lineSpace    float64
}

// NewFont creates new font
func NewFont() *Font {
	f := &Font{
		font: gui.NewQFont2("Inconsolata", 14, int(gui.QFont__Normal), false),
	}

	fontMetrics := gui.NewQFontMetricsF(f.font)
	f.fontMetrics = fontMetrics
	f.height = fontMetrics.Height()
	f.width = fontMetrics.Width("W")
	f.ascent = fontMetrics.Ascent()
	f.descent = fontMetrics.Descent()
	f.underlinePos = fontMetrics.UnderlinePos()
	f.leading = fontMetrics.Leading()

	f.lineSpace = float64(10)
	f.lineHeight = float64(int(f.height + f.lineSpace + 0.5))
	f.shift = float64(int(f.lineSpace/2 + f.ascent - f.leading + 0.5))

	return f
}
