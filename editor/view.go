package editor

import (
	"fmt"

	xi "github.com/dzhou121/xi-go/xi-client"
	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// View displays a buffer
type View struct {
	editor      *Editor
	view        *widgets.QGraphicsView
	scence      *widgets.QGraphicsScene
	scenceLines map[int]*widgets.QGraphicsTextItem
	font        *gui.QFont

	nInvalidBefore int
	lines          []*Line
	nInvalidAfter  int
	revision       int
	height         int

	xiView *xi.View
}

// Line is
type Line struct {
	text string
}

// NewView creates a new view
func NewView(e *Editor) *View {
	view := &View{
		editor:      e,
		font:        gui.NewQFont(),
		view:        widgets.NewQGraphicsView(nil),
		scence:      widgets.NewQGraphicsScene(nil),
		scenceLines: map[int]*widgets.QGraphicsTextItem{},
		lines:       []*Line{},
	}
	e.view = view

	view.view.ConnectKeyPressEvent(func(event *gui.QKeyEvent) {
		if view.xiView == nil {
			return
		}

		if core.Qt__Key(event.Key()) == core.Qt__Key_Return {
			view.xiView.Insert("\n")
			return
		}
		view.xiView.Insert(event.Text())
	})
	view.view.SetScene(view.scence)
	view.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)

	go func() {
		<-e.init
		view.xiView, _ = e.xi.NewView("/Users/Lulu/.config/nvim/init.vim")
	}()
	return view
}

func (v *View) applyUpdate(update *xi.UpdateNotification) {
	inval := [][]int{}
	oldHeight := v.height
	newInvalidBefore := 0
	newLines := []*Line{}
	newInvalidAfter := 0
	oldIx := 0

	for _, op := range update.Update.Ops {
		n := op.N
		switch op.Op {
		case "invalidate":
			curLine := newInvalidBefore + len(newLines) + newInvalidAfter
			ix := curLine - v.nInvalidBefore
			if ix+n > 0 && ix < len(v.lines) {
				for i := Max(ix, 0); i < Min(ix+n, len(v.lines)); i++ {
					if v.lines[i] != nil {
						inval = append(inval, []int{i + v.nInvalidBefore, i + v.nInvalidBefore})
					}
				}
			}
			if len(newLines) == 0 {
				newInvalidBefore += n
			} else {
				newInvalidAfter += n
			}
		case "ins":
			for i := 0; i < newInvalidAfter; i++ {
				newLines = append(newLines, nil)
			}
			newInvalidAfter = 0
			inval = append(inval, []int{newInvalidBefore + len(newLines), newInvalidBefore + len(newLines) + n})
			for _, line := range op.Lines {
				newLines = append(newLines, &Line{
					text: line.Text,
				})
			}
		case "copy", "update":
			n := op.N
			nRemaining := n
			if oldIx < v.nInvalidBefore {
				nInvalid := Min(n, v.nInvalidBefore-oldIx)
				if len(newLines) == 0 {
					newInvalidBefore += nInvalid
				} else {
					newInvalidAfter += nInvalid
				}
				oldIx += nInvalid
				nRemaining -= nInvalid
			}
			if nRemaining > 0 && oldIx < v.nInvalidBefore+len(v.lines) {
				for i := 0; i < newInvalidAfter; i++ {
					newLines = append(newLines, nil)
				}
				newInvalidAfter = 0
				nCopy := Min(nRemaining, v.nInvalidBefore+len(v.lines)-oldIx)
				if oldIx != newInvalidBefore+len(newLines) || op.Op != "copy" {
					inval = append(inval, []int{newInvalidBefore + len(newLines), newInvalidBefore + len(newLines) + nCopy})
				}
				startIx := oldIx - v.nInvalidBefore
				if op.Op == "copy" {
					newLines = append(newLines, v.lines[startIx:startIx+nCopy]...)
				} else {
				}
				oldIx += nCopy
				nRemaining -= nCopy
			}
			if len(newLines) == 0 {
				newInvalidBefore += nRemaining
			} else {
				newInvalidAfter += nRemaining
			}
			oldIx += nRemaining
		case "skip":
			oldIx += n
		default:
			fmt.Println("unknown op type", op.Op)
		}
	}

	v.nInvalidBefore = newInvalidBefore
	v.lines = newLines
	v.nInvalidAfter = newInvalidAfter
	v.revision++

	if v.height < oldHeight {
		inval = append(inval, []int{v.height, oldHeight})
	}
	fmt.Println(inval)
	for _, invalRange := range inval {
		start := invalRange[0]
		end := invalRange[1]
		for i := start; i < end; i++ {
			line := v.lines[i]
			scenceLine := v.getScenceLine(i)
			if line == nil {
				scenceLine.SetPlainText("")
			} else {
				// fmt.Println("set plaintext", i, line.text, start, end)
				scenceLine.SetPlainText(line.text)
			}
		}
	}
}

func (v *View) getScenceLine(i int) *widgets.QGraphicsTextItem {
	fmt.Println("get scecen line", i)
	scenceLine, ok := v.scenceLines[i]
	if ok {
		return scenceLine
	}
	scenceLine = v.scence.AddText("", v.font)
	scenceLine.SetPos2(0, 20*float64(i))
	v.scenceLines[i] = scenceLine
	return scenceLine
}
