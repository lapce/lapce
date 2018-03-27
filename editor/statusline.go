package editor

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/svg"
	"github.com/therecipe/qt/widgets"
)

type statuslineSignal struct {
	core.QObject
	_ func() `signal:"gitSignal"`
}

// StatusMode is
type StatusMode struct {
	s     *StatusLine
	label *widgets.QLabel
	mode  string
	text  string
	bg    *Color
}

// StatuslineGit is
type StatuslineGit struct {
	s         *StatusLine
	branch    string
	file      string
	widget    *widgets.QWidget
	label     *widgets.QLabel
	icon      *svg.QSvgWidget
	svgLoaded bool
	hidden    bool
}

// StatuslineFile is
type StatuslineFile struct {
	s           *StatusLine
	file        string
	fileType    string
	widget      *widgets.QWidget
	fileLabel   *widgets.QLabel
	folderLabel *widgets.QLabel
	icon        *svg.QSvgWidget
	base        string
	dir         string
}

// StatuslineFiletype is
type StatuslineFiletype struct {
	filetype string
	label    *widgets.QLabel
}

// StatuslinePos is
type StatuslinePos struct {
	ln    int
	col   int
	label *widgets.QLabel
	text  string
}

// StatusLine is
type StatusLine struct {
	editor *Editor
	widget *widgets.QWidget
	signal *statuslineSignal
	height int

	mode     *StatusMode
	git      *StatuslineGit
	file     *StatuslineFile
	filetype *StatuslineFiletype
	pos      *StatuslinePos
}

func newStatusLine(editor *Editor) *StatusLine {
	s := &StatusLine{
		editor: editor,
		widget: widgets.NewQWidget(nil, 0),
		height: int(editor.defaultFont.lineHeight),
		signal: NewStatuslineSignal(nil),
	}
	s.widget.SetContentsMargins(0, 1, 0, 0)
	layout := newVFlowLayout(8, 8, 1, 3, 0)
	s.widget.SetLayout(layout)
	s.widget.SetObjectName("statusline")
	s.widget.SetStyleSheet(`
	QWidget#statusline {
		border-top: 1px solid rgba(0, 0, 0, 1);
		background-color: rgba(24, 29, 34, 1);
	}
	* {
		color: rgba(205, 211, 222, 1);
	}
	`)

	modeLabel := widgets.NewQLabel(nil, 0)
	modeLabel.SetContentsMargins(4, 1, 4, 1)
	modeLayout := widgets.NewQHBoxLayout()
	modeLayout.AddWidget(modeLabel, 0, 0)
	modeLayout.SetContentsMargins(0, 0, 0, 0)
	modeWidget := widgets.NewQWidget(nil, 0)
	modeWidget.SetContentsMargins(0, 4, 0, 4)
	modeWidget.SetLayout(modeLayout)
	mode := &StatusMode{
		s:     s,
		label: modeLabel,
	}
	s.mode = mode

	gitIcon := svg.NewQSvgWidget(nil)
	gitIcon.SetFixedSize2(14, 14)
	gitLabel := widgets.NewQLabel(nil, 0)
	gitLabel.SetContentsMargins(0, 0, 0, 0)
	gitLayout := widgets.NewQHBoxLayout()
	gitLayout.SetContentsMargins(0, 0, 0, 0)
	gitLayout.SetSpacing(2)
	gitLayout.AddWidget(gitIcon, 0, 0)
	gitLayout.AddWidget(gitLabel, 0, 0)
	gitWidget := widgets.NewQWidget(nil, 0)
	gitWidget.SetContentsMargins(0, 0, 0, 0)
	gitWidget.SetLayout(gitLayout)
	gitWidget.Hide()
	git := &StatuslineGit{
		s:      s,
		widget: gitWidget,
		icon:   gitIcon,
		label:  gitLabel,
	}
	s.git = git

	filetypeLabel := widgets.NewQLabel(nil, 0)
	filetypeLabel.SetContentsMargins(0, 0, 0, 0)
	filetype := &StatuslineFiletype{
		label: filetypeLabel,
	}
	s.filetype = filetype

	fileIcon := svg.NewQSvgWidget(nil)
	fileIcon.SetFixedSize2(14, 14)
	fileLabel := widgets.NewQLabel(nil, 0)
	fileLabel.SetContentsMargins(0, 0, 0, 0)
	folderLabel := widgets.NewQLabel(nil, 0)
	folderLabel.SetContentsMargins(0, 0, 0, 0)
	folderLabel.SetStyleSheet("color: #838383;")
	folderLabel.SetContentsMargins(0, 0, 0, 0)
	fileLayout := widgets.NewQHBoxLayout()
	fileLayout.SetContentsMargins(0, 0, 0, 0)
	fileLayout.SetSpacing(2)
	fileLayout.AddWidget(fileIcon, 0, 0)
	fileLayout.AddWidget(fileLabel, 0, 0)
	fileLayout.AddWidget(folderLabel, 0, 0)
	fileWidget := widgets.NewQWidget(nil, 0)
	fileWidget.SetContentsMargins(0, 0, 0, 0)
	fileWidget.SetLayout(fileLayout)
	file := &StatuslineFile{
		s:           s,
		icon:        fileIcon,
		widget:      fileWidget,
		fileLabel:   fileLabel,
		folderLabel: folderLabel,
	}
	s.file = file

	posLabel := widgets.NewQLabel(nil, 0)
	posLabel.SetContentsMargins(0, 0, 0, 0)
	pos := &StatuslinePos{
		label: posLabel,
	}
	s.pos = pos

	layout.AddWidget(modeWidget)
	layout.AddWidget(gitWidget)
	layout.AddWidget(fileWidget)
	layout.AddWidget(filetypeLabel)
	layout.AddWidget(posLabel)

	s.signal.ConnectGitSignal(func() {
		s.git.update()
	})

	return s
}

func (s *StatusLine) fileUpdate() {
	win := s.editor.activeWin
	if win == nil {
		return
	}
	file := win.buffer.path
	filetype := filepath.Ext(file)
	if filetype != "" {
		filetype = string(filetype[1:])
	}
	s.file.redraw(file)
	s.filetype.redraw(filetype)
	go s.git.redraw(file)
}

func (s *StatusMode) update() {
	s.label.SetText(s.text)
	s.label.SetStyleSheet(fmt.Sprintf("background-color: %s;", s.bg.String()))
}

func (s *StatusMode) redraw() {
	mode := "normal"
	editor := s.s.editor
	if editor.mode == Normal && editor.selection {
		mode = "visual"
	} else if editor.mode == Insert {
		mode = "insert"
	}
	if mode == s.mode {
		return
	}
	s.mode = mode
	text := s.mode
	bg := newColor(102, 153, 204, 255)
	switch s.mode {
	case "normal":
		text = "normal"
		bg = newColor(102, 153, 204, 255)
	case "cmdline_normal":
		text = "normal"
		bg = newColor(102, 153, 204, 255)
	case "insert":
		text = "insert"
		bg = newColor(153, 199, 148, 255)
	case "visual":
		text = "visual"
		bg = newColor(250, 200, 99, 255)
	}
	s.text = text
	s.bg = bg
	s.update()
}

func (s *StatuslineGit) hide() {
	if s.hidden {
		return
	}
	s.hidden = true
	s.s.signal.GitSignal()
}

func (s *StatuslineGit) update() {
	if s.hidden {
		s.widget.Hide()
		return
	}
	s.label.SetText(s.branch)
	if !s.svgLoaded {
		s.svgLoaded = true
		svgContent := s.s.editor.getSvg("git", newColor(212, 215, 214, 255))
		s.icon.Load2(core.NewQByteArray2(svgContent, len(svgContent)))
	}
	s.widget.Show()
}

func (s *StatuslineGit) redraw(file string) {
	if file == "" || strings.HasPrefix(file, "term://") {
		s.file = file
		s.hide()
		s.branch = ""
		return
	}

	if s.file == file {
		return
	}

	s.file = file
	dir := filepath.Dir(file)
	out, err := exec.Command("git", "-C", dir, "branch").Output()
	if err != nil {
		s.hide()
		s.branch = ""
		return
	}

	branch := ""
	for _, line := range strings.Split(string(out), "\n") {
		if strings.HasPrefix(line, "* ") {
			if strings.HasPrefix(line, "* (HEAD detached at ") {
				branch = line[20 : len(line)-1]
			} else {
				branch = line[2:]
			}
		}
	}
	_, err = exec.Command("git", "-C", dir, "diff", "--quiet").Output()
	if err != nil {
		branch += "*"
	}

	if s.branch != branch {
		s.branch = branch
		s.hidden = false
		s.s.signal.GitSignal()
	}
}

func (s *StatuslineFile) updateIcon() {
	svgContent := s.s.editor.getSvg(s.fileType, nil)
	s.icon.Load2(core.NewQByteArray2(svgContent, len(svgContent)))
}

func (s *StatuslineFile) redraw(file string) {
	if file == "" {
		file = "[No Name]"
	}

	if file == s.file {
		return
	}

	s.file = file

	base := filepath.Base(file)
	dir := filepath.Dir(file)
	if dir == "." {
		dir = ""
	}
	if strings.HasPrefix(file, "term://") {
		base = file
		dir = ""
	}
	fileType := filepath.Ext(file)
	if fileType != "" {
		fileType = string(fileType[1:])
	}
	if s.fileType != fileType {
		s.fileType = fileType
		s.updateIcon()
	}
	if s.base != base {
		s.base = base
		s.fileLabel.SetText(s.base)
	}
	if s.dir != dir {
		s.dir = dir
		s.folderLabel.SetText(s.dir)
	}
}

func (s *StatuslineFiletype) redraw(filetype string) {
	if filetype == s.filetype {
		return
	}
	s.filetype = filetype
	s.label.SetText(s.filetype)
}

func (s *StatuslinePos) redraw(ln, col int) {
	if ln == s.ln && col == s.col {
		return
	}
	text := fmt.Sprintf("Ln %d, Col %d", ln, col)
	if text != s.text {
		s.text = text
		s.label.SetText(text)
	}
}

func newVFlowLayout(spacing int, padding int, paddingTop int, rightIdex int, width int) *widgets.QLayout {
	layout := widgets.NewQLayout2()
	items := []*widgets.QLayoutItem{}
	rect := core.NewQRect()
	layout.ConnectSizeHint(func() *core.QSize {
		size := core.NewQSize()
		for _, item := range items {
			size = size.ExpandedTo(item.MinimumSize())
		}
		return size
	})
	if width > 0 {
		layout.ConnectMinimumSize(func() *core.QSize {
			size := core.NewQSize()
			for _, item := range items {
				size = size.ExpandedTo(item.MinimumSize())
			}
			if size.Width() > width {
				size.SetWidth(width)
			}
			size.SetWidth(0)
			return size
		})
		layout.ConnectMaximumSize(func() *core.QSize {
			size := core.NewQSize()
			for _, item := range items {
				size = size.ExpandedTo(item.MinimumSize())
			}
			size.SetWidth(width)
			return size
		})
	}
	layout.ConnectAddItem(func(item *widgets.QLayoutItem) {
		items = append(items, item)
	})
	layout.ConnectSetGeometry(func(r *core.QRect) {
		x := padding
		right := padding
		sizes := [][]int{}
		maxHeight := 0
		totalWidth := r.Width()
		for _, item := range items {
			sizeHint := item.SizeHint()
			width := sizeHint.Width()
			height := sizeHint.Height()
			size := []int{width, height}
			sizes = append(sizes, size)
			if height > maxHeight {
				maxHeight = height
			}
		}
		for i, item := range items {
			size := sizes[i]
			width := size[0]
			height := size[1]
			y := paddingTop
			if height != maxHeight {
				y = (maxHeight-height)/2 + paddingTop
			}

			if rightIdex > 0 && i >= rightIdex {
				rect.SetRect(totalWidth-width-right, y, width, height)
				item.SetGeometry(rect)
				if width > 0 {
					right += width + spacing
				}
			} else {
				if x+width+padding > totalWidth {
					width = totalWidth - x - padding
					rect.SetRect(x, y, width, height)
					item.SetGeometry(rect)
					break
				}
				rect.SetRect(x, y, width, height)
				item.SetGeometry(rect)
				if width > 0 {
					x += width + spacing
				}
			}
		}
	})
	layout.ConnectItemAt(func(index int) *widgets.QLayoutItem {
		if index < len(items) {
			return items[index]
		}
		return nil
	})
	layout.ConnectTakeAt(func(index int) *widgets.QLayoutItem {
		if index < len(items) {
			return items[index]
		}
		return nil
	})
	return layout
}
