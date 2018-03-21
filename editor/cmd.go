package editor

import "sync"

func (e *Editor) executeKey(key string) {
	keys := e.keymap.lookup(key)
	if keys == nil {
		e.setCmd(key)
		e.states[e.mode].execute()
		return
	}

	for _, key = range keys {
		e.setCmd(key)
		e.states[e.mode].execute()
	}
}

func (e *Editor) setCmd(key string) {
	e.cmdArg.cmd = key
}

func (e *Editor) getCmdCount() int {
	count := 1
	if e.cmdArg.count > 0 {
		count = e.cmdArg.count
	}
	return count
}

func (e *Editor) updateCursorShape() {
	if e.activeWin == nil {
		return
	}
	w, h := e.states[e.mode].cursor()
	e.cursor.Resize2(w, h)
}

func (e *Editor) toInsert() {
	e.mode = Insert
	e.updateCursorShape()
}

func (e *Editor) toNormal() {
	if !e.config.Modal {
		return
	}
	e.mode = Normal
	e.updateCursorShape()
	win := e.activeWin
	if win.col > 0 {
		win.scroll(0, -1, true, false)
	}
}

func (e *Editor) toInsertRight() {
	e.mode = Insert
	e.updateCursorShape()
	win := e.activeWin
	if win.col < len(win.buffer.lines[win.row].text)-1 {
		win.scroll(0, 1, true, false)
	}
}

func (e *Editor) toInsertEndOfLine() {
	e.mode = Insert
	e.updateCursorShape()
	win := e.activeWin
	row := win.row
	maxCol := len(win.buffer.lines[row].text) - 1
	if maxCol < 0 {
		maxCol = 0
	}
	win.scroll(0, maxCol-win.col, true, false)
}

func (e *Editor) toInsertNewLine() {
	e.mode = Insert
	win := e.activeWin
	row := win.row + 1
	col := 0
	win.scrollToCursor(row+1, col, false)
	win.row = row
	win.col = col
	win.buffer.xiView.Click(row, col)
	win.buffer.xiView.InsertNewline()
	win.buffer.xiView.Click(row, col)
}

func (e *Editor) toInsertNewLineAbove() {
	e.mode = Insert
	win := e.activeWin
	row := win.row
	col := 0
	win.scrollToCursor(row, col, false)
	win.buffer.xiView.Click(row, col)
	win.buffer.xiView.InsertNewline()
	win.buffer.xiView.Click(row, col)
}

func (e *Editor) wordEnd() {
	win := e.activeWin
	count := e.getCmdCount()
	row, col := win.wordEnd(count)
	win.scroll(row-win.row, col-win.col, true, false)
}

func (e *Editor) wordForward() {
	count := e.getCmdCount()
	win := e.activeWin
	row, col := win.wordForward(count)
	win.scroll(row-win.row, col-win.col, true, false)
}

func (e *Editor) down() {
	e.activeWin.scroll(e.getCmdCount(), 0, true, false)
}

func (e *Editor) up() {
	e.activeWin.scroll(-e.getCmdCount(), 0, true, false)
}

func (e *Editor) left() {
	e.activeWin.scroll(0, -e.getCmdCount(), true, false)
}

func (e *Editor) right() {
	e.activeWin.scroll(0, e.getCmdCount(), true, false)
}

func (e *Editor) goTo() {
	win := e.activeWin
	row := 0
	maxRow := len(win.buffer.lines) - 1
	if e.cmdArg.count == 0 {
		if e.cmdArg.cmd == "G" {
			row = maxRow
		} else {
			row = 0
		}
	} else {
		row = e.cmdArg.count - 1
		if row > maxRow {
			row = maxRow
		}
	}
	win.scroll(row-win.row, 0, true, false)
}

func (e *Editor) scrollUp() {
	e.activeWin.scroll(-e.getCmdCount(), 0, false, true)
}

func (e *Editor) scrollDown() {
	e.activeWin.scroll(e.getCmdCount(), 0, false, true)

}

func (e *Editor) pageDown() {
	win := e.activeWin
	n := (win.end - win.start) / 2
	win.scroll(n, 0, true, true)
}

func (e *Editor) pageUp() {
	win := e.activeWin
	n := (win.end - win.start) / 2
	win.scroll(-n, 0, true, true)
}

func (e *Editor) startOfLine() {
	win := e.activeWin
	row := win.row
	col := 0
	win.scrollCol = 0
	if e.selection {
		win.buffer.xiView.Drag(row, col)
	} else {
		win.buffer.xiView.Click(row, col)
	}
}

func (e *Editor) endOfLine() {
	win := e.activeWin
	row := win.row
	maxCol := len(win.buffer.lines[row].text) - 2
	if e.selection {
		maxCol++
	}
	if maxCol < 0 {
		maxCol = 0
	}
	win.scrollCol = maxCol
	if e.selection {
		win.buffer.xiView.Drag(row, maxCol)
	} else {
		win.buffer.xiView.Click(row, maxCol)
	}
}

func (e *Editor) undo() {
	e.activeWin.buffer.xiView.Undo()
}

func (e *Editor) redo() {
	e.activeWin.buffer.xiView.Redo()
}

func (e *Editor) search() {
	win := e.activeWin
	buffer := win.buffer
	if e.selection {
		if e.mode == Normal {
			e.states[Normal].(*NormalState).cancelVisual(false)
		}
		buffer.xiView.Find("")
		buffer.xiView.FindNext(true)
		return
	}
	word := win.wordUnderCursor()
	if word != "" {
		buffer.xiView.Find(word)
		buffer.xiView.FindNext(true)
	}
}

func (e *Editor) findNext() {
	e.activeWin.buffer.xiView.FindNext(false)
}

func (e *Editor) delForward() {
	e.activeWin.buffer.xiView.DeleteForward()
	if e.mode == Normal {
		e.states[Normal].(*NormalState).cancelVisual(false)
	}
}

func (e *Editor) verticalSplit() {
	e.activeWin.frame.split(true)
}

func (e *Editor) horizontalSplit() {
	e.activeWin.frame.split(false)
}

func (e *Editor) closeSplit() {
	e.activeWin.frame.close()
}

func (e *Editor) exchangeSplit() {
	e.activeWin.frame.exchange()
}

func (e *Editor) leftSplit() {
	e.activeWin.frame.focusLeft()
}

func (e *Editor) rightSplit() {
	e.activeWin.frame.focusRight()
}

func (e *Editor) aboveSplit() {
	e.activeWin.frame.focusAbove()
}

func (e *Editor) belowSplit() {
	e.activeWin.frame.focusBelow()
}

var cmdPaletteItems []*PaletteItem
var cmdPaletteItemsOnce sync.Once

func (e *Editor) allCmds() []*PaletteItem {
	cmdPaletteItemsOnce.Do(func() {
		items := []*PaletteItem{}
		item := &PaletteItem{
			description: "Vertical Split",
			itemType:    PaletteCmd,
			cmd:         e.verticalSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Horizontal Split",
			itemType:    PaletteCmd,
			cmd:         e.horizontalSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Close Split",
			itemType:    PaletteCmd,
			cmd:         e.closeSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Exchange Split",
			itemType:    PaletteCmd,
			cmd:         e.exchangeSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Left Split",
			itemType:    PaletteCmd,
			cmd:         e.leftSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Right Split",
			itemType:    PaletteCmd,
			cmd:         e.rightSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Above Split",
			itemType:    PaletteCmd,
			cmd:         e.aboveSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Below Split",
			itemType:    PaletteCmd,
			cmd:         e.belowSplit,
		}
		items = append(items, item)

		cmdPaletteItems = items
	})
	return cmdPaletteItems
}

func (e *Editor) commandPalette() {
	e.palette.run(e.allCmds())
}
