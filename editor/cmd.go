package editor

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/crane-editor/crane/log"
)

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
	e.statusLine.mode.redraw()
	for _, w := range e.wins {
		w.gutter.Update()
	}
	w, h := e.states[e.mode].cursor()
	e.cursor.Resize2(w, h)
}

func (e *Editor) toInsert() {
	e.mode = Insert
	e.updateCursorShape()
}

func (e *Editor) toNormal() {
	e.popup.hide()
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
	maxCol := len(win.buffer.lines[win.row].text) - 1
	if maxCol < 0 {
		maxCol = 0
	}
	win.col = maxCol
	win.buffer.xiView.Click(win.row, maxCol)
	win.buffer.xiView.InsertNewline()
	// row := win.row + 1
	// col := 0
	// if win.buffer.lines[win.row] != nil {
	// 	for i, r := range win.buffer.lines[win.row].text {
	// 		if utfClass(r) > 0 {
	// 			col = i
	// 			break
	// 		}
	// 	}
	// }
	// win.scrollToCursor(row+1, col, false)
	// win.row = row
	// win.col = col
	// win.buffer.xiView.Click(row, col)
	// win.buffer.xiView.InsertNewline()
	// win.buffer.xiView.Click(row, col)
}

func (e *Editor) toInsertNewLineAbove() {
	e.mode = Insert
	win := e.activeWin
	row := win.row
	row--
	if row >= 0 {
		maxCol := len(win.buffer.lines[row].text) - 1
		if maxCol < 0 {
			maxCol = 0
		}
		win.row = row
		win.col = maxCol
		win.buffer.xiView.Click(row, maxCol)
		win.buffer.xiView.InsertNewline()
		return
	}

	win.row = 0
	win.col = 0
	win.buffer.xiView.Click(win.row, win.col)
	win.buffer.xiView.InsertNewline()
	win.buffer.xiView.Click(win.row, win.col)
}

func (e *Editor) wordEnd() {
	win := e.activeWin
	count := e.getCmdCount()
	row, col := win.wordEnd(count)
	if e.selection {
		col++
	}
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

func (e *Editor) save() {
	go func() {
		e.lspClient.format(e.activeWin.buffer)
		e.activeWin.buffer.xiView.Save()
	}()
}

func (e *Editor) redo() {
	e.activeWin.buffer.xiView.Redo()
}

func (e *Editor) search() {
	win := e.activeWin
	buffer := win.buffer
	text := ""
	if e.selection {
		text = buffer.xiView.Copy()
		e.states[Normal].(*NormalState).cancelVisual(true)
	} else {
		text = win.wordUnderCursor()
	}
	if text == "" {
		return
	}
	e.findString = text
	e.findNext()
}

func (e *Editor) findNext() {
	if e.findString == "" {
		return
	}
	e.activeWin.buffer.xiView.Find(e.findString)
	e.activeWin.buffer.xiView.FindNext(false)
}

func (e *Editor) findPrevious() {
	if e.findString == "" {
		return
	}
	e.activeWin.buffer.xiView.Find(e.findString)
	e.activeWin.buffer.xiView.FindPrevious(false)
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

func (e *Editor) hover() {
	win := e.activeWin
	e.lspClient.hover(win.buffer, win.row, win.col)
}

func (e *Editor) nextDiagnostic() {
	win := e.activeWin
	diags, ok := e.diagnostics[win.buffer.path]
	if !ok {
		return
	}
	sort.Sort(byLine(diags.Diagnostics))
	row := win.row
	for _, diag := range diags.Diagnostics {
		if diag.Range.Start.Line > row {
			win.buffer.xiView.Click(diag.Range.Start.Line, diag.Range.Start.Character)
			return
		}
	}
}

func (e *Editor) previousDiagnostic() {
	win := e.activeWin
	diags, ok := e.diagnostics[win.buffer.path]
	if !ok {
		return
	}
	sort.Sort(byLine(diags.Diagnostics))
	row := win.row
	for i := len(diags.Diagnostics) - 1; i >= 0; i-- {
		diag := diags.Diagnostics[i]
		if diag.Range.Start.Line < row {
			win.buffer.xiView.Click(diag.Range.Start.Line, diag.Range.Start.Character)
			return
		}
	}
}

func (e *Editor) definition() {
	win := e.activeWin
	e.lspClient.definition(win.buffer, win.row, win.col)
}

func (e *Editor) previousLocation() {
	e.activeWin.previousLocation()
}

func (e *Editor) nextLocation() {
	e.activeWin.nextLocation()
}

func (e *Editor) changeTheme(themeName string) {
	e.themeName = themeName
	e.xi.SetTheme(themeName)
}

func (e *Editor) changeThemePalette() {
	e.palette.run(PaletteThemes)
}

func (e *Editor) increaseSplitHeight() {
	e.activeWin.frame.changeSize(10, false)
}

func (e *Editor) decreaseSplitHeight() {
	e.activeWin.frame.changeSize(-10, false)
}

func (e *Editor) increaseSplitWidth() {
	e.activeWin.frame.changeSize(10, true)
}

func (e *Editor) decreaseSplitWidth() {
	e.activeWin.frame.changeSize(-10, true)
}

var themesPaletteItems []*PaletteItem
var themesPaletteItemsOnce sync.Once

func (e *Editor) allThemes() []*PaletteItem {
	themesPaletteItemsOnce.Do(func() {
		items := []*PaletteItem{}
		for _, theme := range e.themes {
			item := &PaletteItem{
				description: theme,
			}
			items = append(items, item)
		}
		themesPaletteItems = items
	})
	return themesPaletteItems
}

var cmdPaletteItems []*PaletteItem
var cmdPaletteItemsOnce sync.Once

func (e *Editor) allCmds() []*PaletteItem {
	cmdPaletteItemsOnce.Do(func() {
		items := []*PaletteItem{}
		item := &PaletteItem{
			description: "Split: Vertical",
			itemType:    PaletteCmd,
			cmd:         e.verticalSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Horizontal",
			itemType:    PaletteCmd,
			cmd:         e.horizontalSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Close",
			itemType:    PaletteCmd,
			cmd:         e.closeSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Exchange",
			itemType:    PaletteCmd,
			cmd:         e.exchangeSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Left",
			itemType:    PaletteCmd,
			cmd:         e.leftSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Right",
			itemType:    PaletteCmd,
			cmd:         e.rightSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Above",
			itemType:    PaletteCmd,
			cmd:         e.aboveSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Below",
			itemType:    PaletteCmd,
			cmd:         e.belowSplit,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Increase Width",
			itemType:    PaletteCmd,
			cmd:         e.increaseSplitWidth,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Decrease Width",
			itemType:    PaletteCmd,
			cmd:         e.decreaseSplitWidth,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Increase Height",
			itemType:    PaletteCmd,
			cmd:         e.increaseSplitHeight,
		}
		items = append(items, item)

		item = &PaletteItem{
			description: "Split: Decrease Height",
			itemType:    PaletteCmd,
			cmd:         e.decreaseSplitHeight,
		}
		items = append(items, item)

		item = &PaletteItem{
			description:   "Change Theme",
			cmd:           e.changeThemePalette,
			stayInPalette: true,
		}
		items = append(items, item)

		cmdPaletteItems = items
	})
	return cmdPaletteItems
}

func (e *Editor) commandPalette() {
	e.palette.run(":")
}

var filePaletteItems []*PaletteItem
var filePaletteItemsMutext sync.RWMutex

func (e *Editor) getFilePaletteItemsChan() chan *PaletteItem {
	itemsChan := make(chan *PaletteItem, 1000)
	go func() {
		defer close(itemsChan)
		dir, err := os.Getwd()
		if err != nil {
			return
		}
		cwd := dir + "/"
		files, err := ioutil.ReadDir(dir)
		if err != nil {
			return
		}
		folders := []string{}
		for {
			for _, f := range files {
				if f.IsDir() {
					if f.Name() == ".git" {
						continue
					}
					folders = append(folders, filepath.Join(dir, f.Name()))
					continue
				}
				file := filepath.Join(dir, f.Name())
				file = strings.Replace(file, cwd, "", 1)
				item := &PaletteItem{
					description: file,
				}
				select {
				case itemsChan <- item:
				case <-time.After(time.Second):
					return
				}
			}

			for {
				if len(folders) == 0 {
					return
				}
				dir = folders[0]
				folders = folders[1:]
				files, _ = ioutil.ReadDir(dir)
				if len(files) == 0 {
					continue
				} else {
					break
				}
			}
		}
	}()
	return itemsChan
}

func (e *Editor) getFilePaletteItems() []*PaletteItem {
	items := []*PaletteItem{}
	dir, err := os.Getwd()
	if err != nil {
		return items
	}
	cwd := dir + "/"
	files, err := ioutil.ReadDir(dir)
	if err != nil {
		return items
	}
	folders := []string{}
	for {
		for _, f := range files {
			if f.IsDir() {
				if f.Name() == ".git" {
					continue
				}
				folders = append(folders, filepath.Join(dir, f.Name()))
				continue
			}
			file := filepath.Join(dir, f.Name())
			file = strings.Replace(file, cwd, "", 1)
			item := &PaletteItem{
				description: file,
			}
			items = append(items, item)
		}

		for {
			if len(folders) == 0 {
				return items
			}
			dir = folders[0]
			folders = folders[1:]
			files, _ = ioutil.ReadDir(dir)
			if len(files) == 0 {
				continue
			} else {
				break
			}
		}
	}
}

func (e *Editor) searchLines() {
	e.palette.run("/")
}

func (e *Editor) yank() {
	win := e.activeWin
	row := win.row
	col := win.col
	text := win.buffer.xiView.Copy()
	e.register = text
	e.states[Normal].(*NormalState).cancelVisual(true)
	win.buffer.xiView.Click(row, col)
}

func (e *Editor) pasteClipboard() {
	text := e.clipboard.Text(0)
	if text == "" {
		return
	}
	e.activeWin.buffer.xiView.Insert(text)
}

func (e *Editor) paste() {
	if e.register != "" {
		win := e.activeWin
		row := win.row
		col := win.col
		if !e.selection {
			if e.selectionMode == "V" {
				win.buffer.xiView.Click(e.activeWin.row+1, 0)
			} else {
				win.buffer.xiView.Click(row, col+1)
			}
		}
		win.buffer.xiView.Insert(e.register)
		if e.selectionMode == "V" {
			win.buffer.xiView.Click(row+1, 0)
		}
	}
}

func (e *Editor) copy() {
	e.activeWin.buffer.xiView.Copy()
}

func (e *Editor) changePwd() {
	e.palette.run(">")
}

func (e *Editor) quickOpen() {
	e.palette.run("")
}

func (e *Editor) getFoldersPaletteItemsChan() chan *PaletteItem {
	itemsChan := make(chan *PaletteItem, 1000)
	go func() {
		defer close(itemsChan)

		dir := e.homeDir
		paths, _ := ioutil.ReadDir(dir)
		folders := []string{}

		sep := string(filepath.Separator)

		for {
			for _, p := range paths {
				if p.IsDir() {
					folder := filepath.Join(dir, p.Name())
					path := filepath.Join(dir, p.Name())
					path = strings.Replace(path, e.homeDir, "~", 1)
					item := &PaletteItem{
						description: path,
					}
					count := strings.Count(path, sep)
					if count < 3 {
						folders = append(folders, folder)
					} else {
						if strings.HasPrefix(path, "~/go/") && count < 5 {
							folders = append(folders, folder)
						}
					}
					select {
					case itemsChan <- item:
					case <-time.After(time.Second):
						return
					}
				}
			}
			for {
				if len(folders) == 0 {
					return
				}
				dir = folders[0]
				folders = folders[1:]
				paths, _ = ioutil.ReadDir(dir)
				if len(paths) == 0 {
					continue
				} else {
					break
				}
			}
		}
	}()
	return itemsChan
}

func (e *Editor) getCurrentBufferLinePaletteItemsChan() chan *PaletteItem {
	itemsChan := make(chan *PaletteItem, 1000)
	go func() {
		content := e.activeWin.buffer.xiView.GetContents()
		lines := strings.Split(content, "\n")
		log.Infoln("lines", len(lines))
		defer close(itemsChan)
		buffer := e.activeWin.buffer
		for i, line := range buffer.lines {
			content := ""
			if i < len(lines) {
				content = lines[i]
			}
			if line == nil {
				line = &Line{
					text: content,
				}
				buffer.lines[i] = line
			}

			item := &PaletteItem{
				description: fmt.Sprintf("%d %s", i+1, content),
				lineNumber:  i + 1,
				line:        line,
			}
			select {
			case itemsChan <- item:
			case <-time.After(time.Second):
				return
			}
		}
	}()
	return itemsChan
}
