package editor

import (
	"fmt"
	"runtime"
	"strconv"
	"strings"

	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
)

//
const (
	Normal = iota
	Insert
	Replace
)

//
const (
	Nomatch string = "NOMATCH"
	Digit   string = "DIGIT"
)

// VimAction is
type VimAction func(key string)

// VimCommand is
type VimCommand func()

// VimOutcome is
type VimOutcome struct {
	mode   int
	action VimAction
}

// VimState is
type VimState interface {
	execute()
	setCmd(key string)
}

func newVimStates(e *Editor) map[int]VimState {
	states := map[int]VimState{}
	states[Normal] = newVimNormalState(e)
	states[Insert] = newVimInsertState(e)
	return states
}

// NormalState is
type NormalState struct {
	editor       *Editor
	wincmd       bool
	gcmd         bool
	visualActive bool
	cmdArg       *VimCmdArg
	cmds         map[string]VimCommand
}

// VimCmdArg is
type VimCmdArg struct {
	cmd     string
	opcount int // count before an operator
	count   int
}

func newVimNormalState(e *Editor) VimState {
	s := &NormalState{
		editor: e,
		cmdArg: &VimCmdArg{},
	}
	s.cmds = map[string]VimCommand{
		"<Esc>": s.esc,
		"<C-c>": s.esc,
		"i":     s.toInsert,
		"a":     s.toInsertRight,
		"A":     s.toInsertEndOfLine,
		"h":     s.left,
		"l":     s.right,
		"j":     s.down,
		"k":     s.up,
		"0":     s.startOfLine,
		"$":     s.endOfLine,
		"G":     s.goTo,
		"<C-e>": s.scrollDown,
		"<C-y>": s.scrollUp,
		"<C-d>": s.pageDown,
		"<C-u>": s.pageUp,
		"v":     s.visual,
		"u":     s.undo,
		"<C-r>": s.redo,
	}

	return s
}

func (s *NormalState) setCmd(key string) {
	s.cmdArg.cmd = key
}

func (s *NormalState) execute() {
	i, err := strconv.Atoi(s.cmdArg.cmd)
	if err == nil {
		s.cmdArg.count = s.cmdArg.count*10 + i
		if s.cmdArg.count > 0 {
			return
		}
	}

	if !s.wincmd {
		if s.cmdArg.cmd == "<C-w>" {
			s.wincmd = true
			return
		}
	} else {
		s.doWincmd()
		s.reset()
		return
	}

	if !s.gcmd {
		if s.cmdArg.cmd == "g" {
			s.gcmd = true
			return
		}
	} else {
		s.doGcmd()
		s.reset()
		return
	}

	cmd, ok := s.cmds[s.cmdArg.cmd]
	if !ok {
		return
	}
	cmd()
	s.reset()
}

func (s *NormalState) toInsert() {
	s.editor.vimMode = Insert
	s.editor.updateCursorShape()
}

func (s *NormalState) toInsertRight() {
	s.editor.vimMode = Insert
	s.editor.updateCursorShape()
	if s.editor.activeWin.col < len(s.editor.activeWin.buffer.lines[s.editor.activeWin.row].text)-1 {
		s.editor.activeWin.scrollto(s.editor.activeWin.col+1, s.editor.activeWin.row, true)
		s.editor.activeWin.buffer.xiView.MoveRight()
	}
}

func (s *NormalState) toInsertEndOfLine() {
	s.editor.vimMode = Insert
	s.editor.updateCursorShape()
	win := s.editor.activeWin
	row := win.row
	maxCol := len(win.buffer.lines[row].text) - 1
	if maxCol < 0 {
		maxCol = 0
	}
	win.scrollto(maxCol, row, true)
	win.buffer.xiView.Click(row, maxCol)
}

func (s *NormalState) esc() {
	s.cancelVisual()
	s.reset()
}

func (s *NormalState) reset() {
	s.cmdArg.opcount = 0
	s.cmdArg.count = 0
	s.wincmd = false
	s.gcmd = false
}

func (s *NormalState) doGcmd() {
	cmd := s.cmdArg.cmd
	switch cmd {
	case "g":
		s.goTo()
		return
	}
}

func (s *NormalState) doWincmd() {
	cmd := s.cmdArg.cmd
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
		s.editor.activeWin.frame.split(true)
		return
	case "s":
		s.editor.activeWin.frame.split(false)
		return
	case "c":
		s.editor.activeWin.frame.close()
		return
	case "x":
		s.editor.activeWin.frame.exchange()
		return
	}
}

func (s *NormalState) down() {
	count := 1
	if s.cmdArg.count > 0 {
		count = s.cmdArg.count
	}

	win := s.editor.activeWin
	row := win.row
	row += count
	maxRow := len(win.buffer.lines) - 1
	if row > maxRow {
		row = maxRow
	}
	maxCol := len(win.buffer.lines[row].text) - 2
	if maxCol < 0 {
		maxCol = 0
	}
	col := win.scrollCol
	if col > maxCol {
		col = maxCol
	}
	if s.visualActive {
		win.buffer.xiView.Drag(row, col)
	} else {
		win.buffer.xiView.Click(row, col)
	}
}

func (s *NormalState) up() {
	count := 1
	if s.cmdArg.count > 0 {
		count = s.cmdArg.count
	}

	win := s.editor.activeWin
	row := win.row
	row -= count
	if row < 0 {
		row = 0
	}
	maxCol := len(win.buffer.lines[row].text) - 2
	if maxCol < 0 {
		maxCol = 0
	}
	col := win.scrollCol
	if col > maxCol {
		col = maxCol
	}
	if s.visualActive {
		win.buffer.xiView.Drag(row, col)
	} else {
		win.buffer.xiView.Click(row, col)
	}
}

func (s *NormalState) left() {
	count := 1
	if s.cmdArg.count > 0 {
		count = s.cmdArg.count
	}
	win := s.editor.activeWin
	row := s.editor.activeWin.row
	col := s.editor.activeWin.col
	col -= count
	if col < 0 {
		col = 0
	}
	if s.visualActive {
		s.editor.activeWin.buffer.xiView.Drag(row, col)
	} else {
		s.editor.activeWin.buffer.xiView.Click(row, col)
	}
	win.scrollCol = col
}

func (s *NormalState) right() {
	count := 1
	if s.cmdArg.count > 0 {
		count = s.cmdArg.count
	}
	win := s.editor.activeWin
	row := win.row
	col := win.col
	maxCol := len(win.buffer.lines[win.row].text) - 2
	if maxCol < 0 {
		maxCol = 0
	}
	col += count
	if col > maxCol {
		col = maxCol
	}
	if s.visualActive {
		win.buffer.xiView.Drag(row, col)
	} else {
		win.buffer.xiView.Click(row, col)
	}
	win.scrollCol = col
}

func (s *NormalState) goTo() {
	win := s.editor.activeWin
	row := 0
	col := 0
	maxRow := len(win.buffer.lines) - 1
	if s.cmdArg.count == 0 {
		if s.cmdArg.cmd == "G" {
			row = maxRow
		} else {
			row = 0
		}
	} else {
		row = s.cmdArg.count
		if row > maxRow {
			row = maxRow
		}
	}
	if s.visualActive {
		win.buffer.xiView.Drag(row, col)
	} else {
		win.buffer.xiView.Click(row, col)
	}
}

func (s *NormalState) scrollUp() {
	count := 1
	if s.cmdArg.count > 0 {
		count = s.cmdArg.count
	}
	y := int(float64(count)*s.editor.activeWin.buffer.font.lineHeight + 0.5)
	scrollBar := s.editor.activeWin.view.VerticalScrollBar()
	scrollBar.SetValue(scrollBar.Value() - y)
}

func (s *NormalState) scrollDown() {
	count := 1
	if s.cmdArg.count > 0 {
		count = s.cmdArg.count
	}
	y := int(float64(count)*s.editor.activeWin.buffer.font.lineHeight + 0.5)
	scrollBar := s.editor.activeWin.view.VerticalScrollBar()
	scrollBar.SetValue(scrollBar.Value() + y)
}

func (s *NormalState) pageDown() {
	win := s.editor.activeWin
	n := (win.end - win.start) / 2
	row := win.row
	row += n
	win.buffer.xiView.GotoLine(row)
}

func (s *NormalState) pageUp() {
	win := s.editor.activeWin
	n := (win.end - win.start) / 2
	row := win.row
	row -= n
	if row < 0 {
		row = 0
	}
	win.buffer.xiView.GotoLine(row)
}

func (s *NormalState) startOfLine() {
	win := s.editor.activeWin
	row := win.row
	col := 0
	win.scrollCol = 0
	if s.visualActive {
		win.buffer.xiView.Drag(row, col)
	} else {
		win.buffer.xiView.Click(row, col)
	}
}

func (s *NormalState) endOfLine() {
	win := s.editor.activeWin
	row := win.row
	maxCol := len(win.buffer.lines[row].text) - 2
	if maxCol < 0 {
		maxCol = 0
	}
	win.scrollCol = maxCol
	if s.visualActive {
		win.buffer.xiView.Drag(row, maxCol)
	} else {
		win.buffer.xiView.Click(row, maxCol)
	}
}

func (s *NormalState) visual() {
	if s.visualActive {
		s.cancelVisual()
		return
	}
	s.visualActive = true
	s.editor.activeWin.cline.Hide()
}

func (s *NormalState) cancelVisual() {
	if !s.visualActive {
		return
	}
	s.visualActive = false
	s.editor.activeWin.cline.Show()
	s.editor.activeWin.buffer.xiView.CancelOperation()
}

func (s *NormalState) undo() {
	s.editor.activeWin.buffer.xiView.Undo()
}

func (s *NormalState) redo() {
	s.editor.activeWin.buffer.xiView.Redo()
}

// InsertState is
type InsertState struct {
	editor *Editor
	cmdArg *VimCmdArg
	cmds   map[string]VimCommand
}

func newVimInsertState(e *Editor) VimState {
	s := &InsertState{
		editor: e,
		cmdArg: &VimCmdArg{},
	}
	s.cmds = map[string]VimCommand{
		"<Esc>":   s.toNormal,
		"<Tab>":   s.tab,
		"<Enter>": s.newLine,
		"<C-m>":   s.newLine,
		"<C-j>":   s.newLine,
		"<BS>":    s.deleteBackward,
		"<C-h>":   s.deleteBackward,
		"<C-u>":   s.deleteToBeginningOfLine,
		"<Del>":   s.deleteForward,
	}
	return s
}

func (s *InsertState) setCmd(key string) {
	s.cmdArg.cmd = key
}

func (s *InsertState) execute() {
	cmd, ok := s.cmds[s.cmdArg.cmd]
	if !ok {
		if strings.HasPrefix(s.cmdArg.cmd, "<") && strings.HasSuffix(s.cmdArg.cmd, ">") {
			fmt.Println(s.cmdArg.cmd)
			return
		}
		s.editor.activeWin.buffer.xiView.Insert(s.cmdArg.cmd)
		return
	}
	cmd()
}

func (s *InsertState) toNormal() {
	s.editor.vimMode = Normal
	s.editor.updateCursorShape()
	if s.editor.activeWin.col > 0 {
		s.editor.activeWin.scrollto(s.editor.activeWin.col-1, s.editor.activeWin.row, true)
		s.editor.activeWin.buffer.xiView.MoveLeft()
	}
}

func (s *InsertState) tab() {
	s.editor.activeWin.buffer.xiView.InsertTab()
}

func (s *InsertState) newLine() {
	s.editor.activeWin.buffer.xiView.InsertNewline()
}

func (s *InsertState) deleteForward() {
	s.editor.activeWin.buffer.xiView.DeleteForward()
}

func (s *InsertState) deleteBackward() {
	s.editor.activeWin.buffer.xiView.DeleteBackward()
}

func (s *InsertState) deleteToBeginningOfLine() {
	s.editor.activeWin.buffer.xiView.DeleteToBeginningOfLine()
}

func (e *Editor) updateCursorShape() {
	if e.activeWin == nil {
		return
	}
	font := e.activeWin.buffer.font
	if e.vimMode == Insert {
		e.cursor.Resize2(1, int(font.lineHeight+0.5))
	} else {
		e.cursor.Resize2(int(font.width+0.5), int(font.lineHeight+0.5))
	}
}

func (e *Editor) convertKey(keyEvent *gui.QKeyEvent) string {
	key := keyEvent.Key()
	text := keyEvent.Text()
	mod := keyEvent.Modifiers()
	if mod&core.Qt__KeypadModifier > 0 {
		switch core.Qt__Key(key) {
		case core.Qt__Key_Home:
			return fmt.Sprintf("<%sHome>", e.modPrefix(mod))
		case core.Qt__Key_End:
			return fmt.Sprintf("<%sEnd>", e.modPrefix(mod))
		case core.Qt__Key_PageUp:
			return fmt.Sprintf("<%sPageUp>", e.modPrefix(mod))
		case core.Qt__Key_PageDown:
			return fmt.Sprintf("<%sPageDown>", e.modPrefix(mod))
		case core.Qt__Key_Plus:
			return fmt.Sprintf("<%sPlus>", e.modPrefix(mod))
		case core.Qt__Key_Minus:
			return fmt.Sprintf("<%sMinus>", e.modPrefix(mod))
		case core.Qt__Key_multiply:
			return fmt.Sprintf("<%sMultiply>", e.modPrefix(mod))
		case core.Qt__Key_division:
			return fmt.Sprintf("<%sDivide>", e.modPrefix(mod))
		case core.Qt__Key_Enter:
			return fmt.Sprintf("<%sEnter>", e.modPrefix(mod))
		case core.Qt__Key_Period:
			return fmt.Sprintf("<%sPoint>", e.modPrefix(mod))
		case core.Qt__Key_0:
			return fmt.Sprintf("<%s0>", e.modPrefix(mod))
		case core.Qt__Key_1:
			return fmt.Sprintf("<%s1>", e.modPrefix(mod))
		case core.Qt__Key_2:
			return fmt.Sprintf("<%s2>", e.modPrefix(mod))
		case core.Qt__Key_3:
			return fmt.Sprintf("<%s3>", e.modPrefix(mod))
		case core.Qt__Key_4:
			return fmt.Sprintf("<%s4>", e.modPrefix(mod))
		case core.Qt__Key_5:
			return fmt.Sprintf("<%s5>", e.modPrefix(mod))
		case core.Qt__Key_6:
			return fmt.Sprintf("<%s6>", e.modPrefix(mod))
		case core.Qt__Key_7:
			return fmt.Sprintf("<%s7>", e.modPrefix(mod))
		case core.Qt__Key_8:
			return fmt.Sprintf("<%s8>", e.modPrefix(mod))
		case core.Qt__Key_9:
			return fmt.Sprintf("<%s9>", e.modPrefix(mod))
		}
	}

	if text == "<" {
		return "<lt>"
	}

	specialKey, ok := e.specialKeys[core.Qt__Key(key)]
	if ok {
		return fmt.Sprintf("<%s%s>", e.modPrefix(mod), specialKey)
	}

	if text == "\\" {
		return fmt.Sprintf("<%s%s>", e.modPrefix(mod), "Bslash")
	}

	c := ""
	if mod&e.controlModifier > 0 || mod&e.cmdModifier > 0 {
		if int(e.keyControl) == key || int(e.keyCmd) == key || int(e.keyAlt) == key || int(e.keyShift) == key {
			return ""
		}
		c = string(key)
		if !(mod&e.shiftModifier > 0) {
			c = strings.ToLower(c)
		}
	} else {
		c = text
	}

	if c == "" {
		return ""
	}

	char := core.NewQChar11(c)
	if char.Unicode() < 0x100 && !char.IsNumber() && char.IsPrint() {
		mod &= ^e.shiftModifier
	}

	prefix := e.modPrefix(mod)
	if prefix != "" {
		return fmt.Sprintf("<%s%s>", prefix, c)
	}

	return c
}

func (e *Editor) modPrefix(mod core.Qt__KeyboardModifier) string {
	prefix := ""
	if runtime.GOOS == "linux" || runtime.GOOS == "darwin" {
		if mod&e.cmdModifier > 0 {
			prefix += "D-"
		}
	}

	if mod&e.controlModifier > 0 {
		prefix += "C-"
	}

	if mod&e.shiftModifier > 0 {
		prefix += "S-"
	}

	if mod&e.altModifier > 0 {
		prefix += "A-"
	}

	return prefix
}

func (e *Editor) initSpecialKeys() {
	e.specialKeys = map[core.Qt__Key]string{}
	e.specialKeys[core.Qt__Key_Up] = "Up"
	e.specialKeys[core.Qt__Key_Down] = "Down"
	e.specialKeys[core.Qt__Key_Left] = "Left"
	e.specialKeys[core.Qt__Key_Right] = "Right"

	e.specialKeys[core.Qt__Key_F1] = "F1"
	e.specialKeys[core.Qt__Key_F2] = "F2"
	e.specialKeys[core.Qt__Key_F3] = "F3"
	e.specialKeys[core.Qt__Key_F4] = "F4"
	e.specialKeys[core.Qt__Key_F5] = "F5"
	e.specialKeys[core.Qt__Key_F6] = "F6"
	e.specialKeys[core.Qt__Key_F7] = "F7"
	e.specialKeys[core.Qt__Key_F8] = "F8"
	e.specialKeys[core.Qt__Key_F9] = "F9"
	e.specialKeys[core.Qt__Key_F10] = "F10"
	e.specialKeys[core.Qt__Key_F11] = "F11"
	e.specialKeys[core.Qt__Key_F12] = "F12"
	e.specialKeys[core.Qt__Key_F13] = "F13"
	e.specialKeys[core.Qt__Key_F14] = "F14"
	e.specialKeys[core.Qt__Key_F15] = "F15"
	e.specialKeys[core.Qt__Key_F16] = "F16"
	e.specialKeys[core.Qt__Key_F17] = "F17"
	e.specialKeys[core.Qt__Key_F18] = "F18"
	e.specialKeys[core.Qt__Key_F19] = "F19"
	e.specialKeys[core.Qt__Key_F20] = "F20"
	e.specialKeys[core.Qt__Key_F21] = "F21"
	e.specialKeys[core.Qt__Key_F22] = "F22"
	e.specialKeys[core.Qt__Key_F23] = "F23"
	e.specialKeys[core.Qt__Key_F24] = "F24"
	e.specialKeys[core.Qt__Key_Backspace] = "BS"
	e.specialKeys[core.Qt__Key_Delete] = "Del"
	e.specialKeys[core.Qt__Key_Insert] = "Insert"
	e.specialKeys[core.Qt__Key_Home] = "Home"
	e.specialKeys[core.Qt__Key_End] = "End"
	e.specialKeys[core.Qt__Key_PageUp] = "PageUp"
	e.specialKeys[core.Qt__Key_PageDown] = "PageDown"

	e.specialKeys[core.Qt__Key_Return] = "Enter"
	e.specialKeys[core.Qt__Key_Enter] = "Enter"
	e.specialKeys[core.Qt__Key_Tab] = "Tab"
	e.specialKeys[core.Qt__Key_Backtab] = "Tab"
	e.specialKeys[core.Qt__Key_Escape] = "Esc"

	e.specialKeys[core.Qt__Key_Backslash] = "Bslash"
	e.specialKeys[core.Qt__Key_Space] = "Space"

	goos := runtime.GOOS
	e.shiftModifier = core.Qt__ShiftModifier
	e.altModifier = core.Qt__AltModifier
	e.keyAlt = core.Qt__Key_Alt
	e.keyShift = core.Qt__Key_Shift
	if goos == "darwin" {
		e.controlModifier = core.Qt__MetaModifier
		e.cmdModifier = core.Qt__ControlModifier
		e.metaModifier = core.Qt__AltModifier
		e.keyControl = core.Qt__Key_Meta
		e.keyCmd = core.Qt__Key_Control
	} else {
		e.controlModifier = core.Qt__ControlModifier
		e.metaModifier = core.Qt__MetaModifier
		e.keyControl = core.Qt__Key_Control
		if goos == "linux" {
			e.cmdModifier = core.Qt__MetaModifier
			e.keyCmd = core.Qt__Key_Meta
		}
	}
}
