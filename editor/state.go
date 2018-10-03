package editor

import (
	"fmt"
	"strconv"
	"strings"
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
	editor       *Editor
	wincmd       bool
	gcmd         bool
	visualActive bool
	visualMode   string
	cmds         map[string]Command
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
		"<C-k>": e.hover,
		"<C-n>": e.definition,
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
		"<C-e>": e.scrollDown,
		"<C-y>": e.scrollUp,
		"<C-d>": e.pageDown,
		"<C-u>": e.pageUp,
		"<D-s>": e.save,
		"v":     s.visual,
		"V":     s.visual,
		"u":     e.undo,
		"<C-r>": e.redo,
		"*":     e.search,
		"#":     e.searchLines,
		"n":     e.findNext,
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

	cmd, ok := s.cmds[cmdArg.cmd]
	if !ok {
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
		s.editor.activeWin.frame.focusRight()
		return
	case "h":
		s.editor.activeWin.frame.focusLeft()
		return
	case "k":
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
		s.editor.activeWin.frame.changeSize(-count, true)
		return
	case ">":
		s.editor.activeWin.frame.changeSize(count, true)
		return
	case "+":
		s.editor.activeWin.frame.changeSize(count, false)
		return
	case "-":
		s.editor.activeWin.frame.changeSize(-count, false)
		return
	}
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
	s.editor.updateCursorShape()
	win.cline.Hide()
	win.buffer.xiView.Click(win.row, win.col)
}

func (s *NormalState) cancelVisual(sendToXi bool) {
	if !s.visualActive {
		return
	}
	win := s.editor.activeWin
	s.visualActive = false
	s.editor.selection = false
	s.visualMode = ""
	s.editor.updateCursorShape()
	s.editor.activeWin.cline.Show()
	if sendToXi {
		win.buffer.xiView.CancelOperation()
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
