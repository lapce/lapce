package editor

import (
	"io/ioutil"
	"path/filepath"
	"strings"

	"github.com/therecipe/qt/core"
	"github.com/therecipe/qt/gui"
	"github.com/therecipe/qt/widgets"
)

// Explorer is
type Explorer struct {
	editor       *Editor
	font         *Font
	rect         *core.QRectF
	widget       *widgets.QWidget
	scence       *widgets.QGraphicsScene
	view         *widgets.QGraphicsView
	fileNode     *FileNode
	nodeList     []*FileNode
	row          int
	scenceWidth  int
	scenceHeight int

	width  int
	height int
}

// FileNode is
type FileNode struct {
	level    int
	name     string
	isDir    bool
	children []*FileNode
	expanded bool
	parent   string
	width    int
	row      int
}

func newExplorer(editor *Editor) *Explorer {
	e := &Explorer{
		editor: editor,
		widget: widgets.NewQWidget(nil, 0),
		font:   editor.defaultFont,
		scence: widgets.NewQGraphicsScene(nil),
		view:   widgets.NewQGraphicsView(nil),
		rect:   core.NewQRectF(),
	}
	e.scence.AddWidget(e.widget, 0).SetPos2(0, 0)
	e.view.SetAlignment(core.Qt__AlignLeft | core.Qt__AlignTop)
	e.view.SetFrameStyle(0)
	e.view.SetScene(e.scence)
	e.view.ConnectResizeEvent(func(event *gui.QResizeEvent) {
		// e.view.Hide()
		// e.view.Show()
		// e.view.Update()
		e.refresh()
	})
	e.widget.SetFixedSize2(300, 1000)
	e.widget.ConnectPaintEvent(e.paint)
	e.rect.SetWidth(300)
	e.rect.SetHeight(1000)
	e.scence.SetSceneRect(e.rect)
	e.scence.ConnectMousePressEvent(func(event *widgets.QGraphicsSceneMouseEvent) {
		scencePos := event.ScenePos()
		y := scencePos.Y()
		row := int(y / e.font.lineHeight)
		if row >= len(e.nodeList) {
			return
		}
		e.row = row
		e.toggleExpand()
	})

	go e.resetFileNode()

	return e
}

func (e *Explorer) toggleExpand() {
	if e.row < 0 || e.row >= len(e.nodeList) {
		return
	}

	node := e.nodeList[e.row]
	if !node.isDir {
		e.view.Update()
		e.editor.activeWin.openFile(filepath.Join(node.parent, node.name))
		e.editor.gadgetFocus = ""
		return
	}
	if node.expanded {
		node.expanded = false
		e.refresh()
		return
	}

	e.expandNode(node)
	e.refresh()
}

func (e *Explorer) refresh() {
	e.nodeList = []*FileNode{}
	e.fillList(e.fileNode)
	n := len(e.nodeList)
	width := 0
	for _, node := range e.nodeList {
		if node.width > width {
			width = node.width
		}
	}
	height := int(float64(n)*e.font.lineHeight + 0.5)
	verticalScrollBar := e.view.VerticalScrollBar()
	scrollBarWidth := 0
	if height > e.view.Height() {
		scrollBarWidth = verticalScrollBar.Width()
	}
	viewWidth := e.view.Width() - scrollBarWidth - 1
	if width < viewWidth {
		width = viewWidth
	}
	if width == e.scenceWidth && height == e.scenceHeight {
		return
	}
	e.scenceWidth = width
	e.scenceHeight = height
	e.widget.SetFixedSize2(width, height)
	// e.rect.SetX(0)
	// e.rect.SetY(0)
	// e.rect.SetWidth(float64(width))
	// e.rect.SetHeight(float64(height))
	e.view.SetSceneRect2(0, 0, float64(width), float64(height))
}

func (e *Explorer) changeSize(count int) {
	e.width += count
	if e.width < 10 {
		e.width = 10
	}
	e.editor.centralSplitter.SetSizes([]int{e.width, e.editor.width - e.width})
	e.view.Hide()
	e.view.Show()
}

func (e *Explorer) goToRow(row int) {
	e.row = row
	e.view.EnsureVisible2(
		0,
		float64(e.row)*e.font.lineHeight,
		1,
		e.font.lineHeight,
		20,
		20,
	)
	e.widget.Update()
}

func (e *Explorer) pageUp() {
	n := int(float64(e.editor.height) / e.font.lineHeight / 2)
	e.up(n)
}

func (e *Explorer) pageDown() {
	n := int(float64(e.editor.height) / e.font.lineHeight / 2)
	e.down(n)
}

func (e *Explorer) up(count int) {
	e.row -= count
	if e.row < 0 {
		e.row = 0
	}
	e.goToRow(e.row)
}

func (e *Explorer) down(count int) {
	e.row += count
	if e.row > len(e.nodeList)-1 {
		e.row = len(e.nodeList) - 1
	}
	e.goToRow(e.row)
}

func (e *Explorer) expandNode(node *FileNode) {
	if !node.isDir {
		return
	}
	node.expanded = true
	folder := filepath.Join(node.parent, node.name)

	nodes := []*FileNode{}
	paths, _ := ioutil.ReadDir(folder)
	for _, path := range paths {
		if path.IsDir() {
			node := &FileNode{
				level:    node.level + 1,
				name:     path.Name(),
				isDir:    path.IsDir(),
				expanded: false,
				parent:   folder,
				width:    int(float64(node.level+1)*(5+e.font.height) + e.font.height*2 + 10 + e.font.fontMetrics.Size(0, path.Name(), 0, 0).Rwidth() + 0.5),
			}
			nodes = append(nodes, node)
		}
	}
	for _, path := range paths {
		if !path.IsDir() {
			node := &FileNode{
				level:    node.level + 1,
				name:     path.Name(),
				isDir:    path.IsDir(),
				expanded: false,
				parent:   folder,
				width:    int(float64(node.level+1)*(5+e.font.height) + e.font.height*2 + 10 + e.font.fontMetrics.Size(0, path.Name(), 0, 0).Rwidth() + 0.5),
			}
			nodes = append(nodes, node)
		}
	}
	node.children = nodes
}

func (e *Explorer) fillList(node *FileNode) {
	node.row = len(e.nodeList)
	e.nodeList = append(e.nodeList, node)

	if node.expanded {
		for _, child := range node.children {
			e.fillList(child)
		}
	}
}

func (e *Explorer) getNumber(node *FileNode, width int) (int, int) {
	e.nodeList = append(e.nodeList, node)
	if node.width > width {
		width = node.width
	}
	if node.expanded {
		i := 1
		for _, child := range node.children {
			n, nodeWidth := e.getNumber(child, width)
			i += n
			if nodeWidth > width {
				width = nodeWidth
			}
		}
		return i, width
	}
	return 1, width
}

func (e *Explorer) resetFileNode() {
	e.fileNode = &FileNode{
		level:    -1,
		parent:   e.editor.cwd,
		name:     "",
		isDir:    true,
		expanded: false,
	}

	e.expandNode(e.fileNode)
	e.refresh()
}

func (e *Explorer) paint(event *gui.QPaintEvent) {
	rect := event.M_rect()
	x := rect.X()
	y := rect.Y()
	width := rect.Width()
	height := rect.Height()

	painter := gui.NewQPainter2(e.widget)
	defer painter.DestroyQPainter()

	painter.SetFont(e.font.font)
	// fg := e.editor.theme.Theme.Foreground
	painter.FillRect5(x, y, width, height,
		gui.NewQColor3(24, 29, 34, 255))

	lineHeight := e.editor.theme.Theme.LineHighlight
	lineHeightColor := gui.NewQColor3(lineHeight.R, lineHeight.G, lineHeight.B, lineHeight.A)
	painter.FillRect5(
		0, e.row*int(e.font.lineHeight), e.scenceWidth, int(e.font.lineHeight),
		lineHeightColor,
	)

	penColor := gui.NewQColor3(205, 211, 222, 255)
	painter.SetPen2(penColor)

	node := e.fileNode
	i := 0
	y = i*int(e.font.lineHeight) + int(e.font.shift)
	painter.DrawText3(5, y, strings.Replace(node.parent, e.editor.homeDir, "~", 1))
	i++

	for _, node := range e.nodeList {
		e.paintSingleNode(painter, node, node.row)
	}
}

func (e *Explorer) paintSingleNode(painter *gui.QPainter, node *FileNode, i int) {
	r := core.NewQRectF()
	svg := e.editor.getSvgRenderer("default", nil)

	padding := float64(node.level) * (6 + e.font.height)

	y := int(float64(i)*e.font.lineHeight + e.font.shift)
	if node.isDir {
		if node.expanded {
			svg = e.editor.getSvgRenderer("caret-down", nil)
		} else {
			svg = e.editor.getSvgRenderer("caret-right", nil)
		}
		r.SetX(padding + 4)
		r.SetY(float64(i)*e.font.lineHeight + (e.font.lineSpace / 2))
		r.SetWidth(e.font.height)
		r.SetHeight(e.font.height)
		svg.Render2(painter, r)
	}

	if node.isDir {
		if node.expanded {
			svg = e.editor.getSvgRenderer("folder-open", nil)
		} else {
			svg = e.editor.getSvgRenderer("folder", nil)
		}
	} else {
		syntax := filepath.Ext(node.name)
		if strings.HasPrefix(syntax, ".") {
			syntax = string(syntax[1:])
		}
		svg = e.editor.getSvgRenderer(syntax, nil)
	}
	r.SetX(padding + 6 + e.font.height)
	r.SetY(float64(i)*e.font.lineHeight + (e.font.lineSpace / 2))
	r.SetWidth(e.font.height)
	r.SetHeight(e.font.height)
	svg.Render2(painter, r)

	penColor := gui.NewQColor3(205, 211, 222, 255)
	painter.SetPen2(penColor)
	painter.DrawText3(int(padding+e.font.height*2+12), y, node.name)
}
