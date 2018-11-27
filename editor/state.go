package editor

import (
	"fmt"
	"strconv"
	"strings"

	xi "github.com/crane-editor/crane/xi-client"
)

//
const (
	Normal = iota
	Insert
)

//
const (
	Nomatch string = "NOMATCH"
	Digit   string = "DIGIT"
)

// Command is
type Command func()

// State is
type State interface {
	execute()
	cursor() (int, int)
}

func newStates(e *Editor) map[int]State {
	states := map[int]State{}
	states[Normal] = newNormalState(e)
	states[Insert] = newInsertState(e)
	return states
}

// NormalState is
type NormalState struct {
	editor           *Editor
	wincmd           bool
	gcmd             bool
	zcmd             bool
	searchInLineOn   bool
	searchInLineChar string
	visualActive     bool
	visualMode       string
	cmds             map[string]Command
	lastCmd          []string
}

// CmdArg is
type CmdArg struct {
	cmd   string
	count int
}

func newNormalState(e *Editor) State {
	s := &NormalState{
		editor: e,
	}
	s.cmds = map[string]Command{
		"<Esc>": s.esc,
		"<C-c>": s.esc,
		":":     e.commandPalette,
		"<C-p>": e.quickOpen,
		"<C-;>": e.changePwd,
		"<C-n>": e.definition,
		"<C-j>": e.nextDiagnostic,
		"<C-k>": e.previousDiagnostic,
		"<C-i>": e.nextLocation,
		"<C-o>": e.previousLocation,
		"i":     e.toInsert,
		"a":     e.toInsertRight,
		"A":     e.toInsertEndOfLine,
		"o":     e.toInsertNewLine,
		"O":     e.toInsertNewLineAbove,
		"e":     e.wordEnd,
		"b":     e.wordForward,
		"h":     e.left,
		"l":     e.right,
		"j":     e.down,
		"k":     e.up,
		"0":     e.startOfLine,
		"$":     e.endOfLine,
		"G":     e.goTo,
		"y":     e.yank,
		"p":     e.paste,
		"<D-y>": e.copyClipboard,
		"<D-p>": e.pasteClipboard,
		"<C-e>": e.scrollDown,
		"<C-y>": e.scrollUp,
		"<C-d>": e.pageDown,
		"<C-u>": e.pageUp,
		"<D-s>": e.save,
		"<D-e>": e.focusExplorer,
		"<D-d>": e.focusDiagnostics,
		"v":     s.visual,
		"V":     s.visual,
		"f":     s.searchInLine,
		";":     s.searchInLineNext,
		",":     s.searchInLinePrevious,
		".":     s.repeatCmd,
		"u":     e.undo,
		"<C-r>": e.redo,
		"#":     e.globalSearch,
		"*":     e.search,
		"/":     e.searchLines,
		"n":     e.findNext,
		"N":     e.findPrevious,
		"x":     e.delForward,
		"s":     s.substitute,
	}

	return s
}

func (s *NormalState) cursor() (int, int) {
	font := s.editor.activeWin.buffer.font
	width := int(font.width + 0.5)
	height := int(font.lineHeight + 0.5)
	if s.visualActive {
		width = 1
	}
	return width, height
}

func (s *NormalState) execute() {
	cmdArg := s.editor.cmdArg
	if s.searchInLineOn {
		s.searchInLineChar = cmdArg.cmd
		s.editor.searchInLineNext(s.searchInLineChar)
		s.searchInLineOn = false
		return
	}

	i, err := strconv.Atoi(cmdArg.cmd)
	if err == nil {
		cmdArg.count = cmdArg.count*10 + i
		if cmdArg.count > 0 {
			return
		}
	}

	if !s.wincmd {
		if cmdArg.cmd == "<C-w>" {
			s.wincmd = true
			return
		}
	} else {
		s.doWincmd()
		s.reset()
		return
	}

	if !s.gcmd {
		if cmdArg.cmd == "g" {
			s.gcmd = true
			return
		}
	} else {
		s.doGcmd()
		s.reset()
		return
	}

	if !s.zcmd {
		if cmdArg.cmd == "z" {
			s.zcmd = true
			return
		}
	} else {
		s.doZcmd()
		s.reset()
		return
	}

	cmd, ok := s.cmds[cmdArg.cmd]
	if !ok {
		fmt.Println("unhandled cmd", cmdArg.cmd)
		return
	}
	cmd()
	s.reset()
}

func (s *NormalState) esc() {
	s.cancelVisual(true)
	s.reset()
}

func (s *NormalState) reset() {
	s.editor.cmdArg.count = 0
	s.wincmd = false
	s.gcmd = false
	s.zcmd = false
}

func (s *NormalState) doZcmd() {
	cmd := s.editor.cmdArg.cmd
	switch cmd {
	case "z":
		win := s.editor.activeWin
		x, y := win.buffer.getPos(win.row, win.col)
		win.view.CenterOn2(float64(x), float64(y))
		win.setPos(win.row, win.col, false)
		return
	}
}

func (s *NormalState) doGcmd() {
	cmd := s.editor.cmdArg.cmd
	switch cmd {
	case "g":
		s.editor.goTo()
		return
	}
}

func (s *NormalState) doWincmd() {
	cmd := s.editor.cmdArg.cmd
	count := s.editor.getCmdCount()
	switch cmd {
	case "l":
		if s.editor.gadgetFocus == ExplorerFocus {
			s.editor.gadgetFocus = ""
			return
		}
		s.editor.activeWin.frame.focusRight()
		return
	case "h":
		s.editor.activeWin.frame.focusLeft()
		return
	case "k":
		if s.editor.gadgetFocus == DiagnosticsFocus {
			s.editor.gadgetFocus = ""
			return
		}
		s.editor.activeWin.frame.focusAbove()
		return
	case "j":
		s.editor.activeWin.frame.focusBelow()
		return
	case "v":
		s.editor.verticalSplit()
		return
	case "s":
		s.editor.horizontalSplit()
		return
	case "c":
		s.editor.closeSplit()
		return
	case "x":
		s.editor.exchangeSplit()
		return
	case "<lt>":
		if s.editor.gadgetFocus == ExplorerFocus {
			s.editor.explorer.changeSize(-count)
		} else {
			s.editor.activeWin.frame.changeSize(-count, true)
		}
		return
	case ">":
		if s.editor.gadgetFocus == ExplorerFocus {
			s.editor.explorer.changeSize(count)
		} else {
			s.editor.activeWin.frame.changeSize(count, true)
		}
		return
	case "+":
		s.editor.activeWin.frame.changeSize(count, false)
		return
	case "-":
		s.editor.activeWin.frame.changeSize(-count, false)
		return
	}
}

func (s *NormalState) repeatCmd() {
	if len(s.lastCmd) == 0 {
		return
	}
	for _, cmd := range s.lastCmd {
		s.editor.setCmd(cmd)
		s.execute()
	}
}

func (s *NormalState) searchInLinePrevious() {
	if s.searchInLineChar == "" {
		return
	}
	s.editor.searchInLinePrevious(s.searchInLineChar)
}

func (s *NormalState) searchInLineNext() {
	if s.searchInLineChar == "" {
		return
	}
	s.editor.searchInLineNext(s.searchInLineChar)
}

func (s *NormalState) searchInLine() {
	s.searchInLineOn = true
}

func (s *NormalState) visual() {
	if s.visualActive {
		s.cancelVisual(true)
		return
	}
	win := s.editor.activeWin
	s.visualActive = true
	s.editor.selection = true
	s.visualMode = s.editor.cmdArg.cmd
	s.editor.selectionMode = s.editor.cmdArg.cmd
	s.editor.updateCursorShape()
	win.cline.Hide()
	if s.visualMode == "V" {
		win.buffer.xiView.Gesture(win.row, win.col, xi.MultiLineSelect)
	} else {
		win.buffer.xiView.Gesture(win.row, win.col, xi.RangeSelect)
	}
}

func (s *NormalState) cancelVisual(sendToXi bool) {
	if !s.visualActive {
		return
	}
	win := s.editor.activeWin
	s.visualActive = false
	s.editor.selection = false
	s.editor.updateCursorShape()
	s.editor.activeWin.cline.Show()
	if sendToXi {
		win.buffer.xiView.Gesture(win.row, win.col, xi.PointSelect)
	}
}

func (s *NormalState) substitute() {
	s.editor.delForward()
	s.editor.toInsert()
}

// InsertState is
type InsertState struct {
	editor *Editor
	cmds   map[string]Command
}

func newInsertState(e *Editor) State {
	s := &InsertState{
		editor: e,
	}
	s.cmds = map[string]Command{
		"<Esc>":    e.toNormal,
		"<Tab>":    s.tab,
		"<C-f>":    e.right,
		"<Right>":  e.right,
		"<C-b>":    e.left,
		"<Left>":   e.left,
		"<Up>":     e.up,
		"<C-p>":    e.up,
		"<Down>":   e.down,
		"<C-n>":    e.down,
		"<Enter>":  s.newLine,
		"<C-m>":    s.newLine,
		"<C-j>":    s.newLine,
		"<Space>":  s.insertSpace,
		"<lt>":     s.insertLt,
		"<Bslash>": s.insertBslash,
		"<BS>":     s.deleteBackward,
		"<C-h>":    s.deleteBackward,
		"<C-w>":    s.deleteWordBackward,
		"<C-u>":    s.deleteToBeginningOfLine,
		"<Del>":    s.deleteForward,
		"<D-p>":    e.pasteClipboard,
	}
	return s
}

func (s *InsertState) cursor() (int, int) {
	font := s.editor.activeWin.buffer.font
	height := int(font.lineHeight + 0.5)
	return 1, height
}

func (s *InsertState) execute() {
	cmdArg := s.editor.cmdArg
	cmd, ok := s.cmds[cmdArg.cmd]
	if !ok {
		if strings.HasPrefix(cmdArg.cmd, "<") && strings.HasSuffix(cmdArg.cmd, ">") {
			fmt.Println(cmdArg.cmd)
			return
		}
		s.editor.activeWin.buffer.xiView.Insert(cmdArg.cmd)
		return
	}
	cmd()
}

func (s *InsertState) tab() {
	s.editor.activeWin.buffer.xiView.InsertTab()
}

func (s *InsertState) newLine() {
	s.editor.activeWin.buffer.xiView.InsertNewline()
}

func (s *InsertState) insertSpace() {
	s.editor.activeWin.buffer.xiView.Insert(" ")
}

func (s *InsertState) insertLt() {
	s.editor.activeWin.buffer.xiView.Insert("<")
}

func (s *InsertState) insertBslash() {
	s.editor.activeWin.buffer.xiView.Insert("\\")
}

func (s *InsertState) deleteForward() {
	s.editor.activeWin.buffer.xiView.DeleteForward()
}

func (s *InsertState) deleteBackward() {
	s.editor.activeWin.buffer.xiView.DeleteBackward()
}

func (s *InsertState) deleteWordBackward() {
	s.editor.activeWin.buffer.xiView.DeleteWordBackward()
}

func (s *InsertState) deleteToBeginningOfLine() {
	if s.editor.activeWin.col == 0 {
		s.editor.activeWin.buffer.xiView.DeleteBackward()
	} else {
		s.editor.activeWin.buffer.xiView.DeleteToBeginningOfLine()
	}
}
