package plugin

import (
	"strings"
	"unsafe"
)

// Line is
type Line struct {
	Text   string
	length int
}

// LineCache is
type LineCache struct {
	NbLines int
	ViewID  string
	Lines   []*Line
	Raw     []byte
}

// View is
type View struct {
	Rev       uint64
	Row       int
	Col       int
	Offset    int
	ID        string
	Path      string
	Syntax    string
	LineCache *LineCache
	Cache     *Cache
}

// SetRaw sets
func (v *View) SetRaw(raw []byte) {
	l := v.LineCache
	l.Raw = raw
	lines := []*Line{}
	lineRunes := []rune{}
	for _, c := range []rune(*(*string)(unsafe.Pointer(&raw))) {
		lineRunes = append(lineRunes, c)
		if c == '\n' {
			line := &Line{
				Text: string(lineRunes),
			}
			line.length = len(line.Text)
			lines = append(lines, line)
			lineRunes = []rune{}
		}
	}
	if len(lineRunes) > 0 {
		line := &Line{
			Text: string(lineRunes),
		}
		line.length = len(line.Text)
		lines = append(lines, line)
	}
	l.Lines = lines
}

// ApplyUpdate applies update
func (v *View) ApplyUpdate(update *Update) (int, int, int, int, string, string, bool) {
	l := v.LineCache
	v.Rev = update.Rev
	i := 0
	startCopy := update.Delta.Els[i].Copy
	startOffset := 0
	if startCopy != nil {
		i++
		startOffset = startCopy[1]
	}
	startRow, startCol := v.GetPos(startOffset)
	text := ""
	deletedText := ""
	if i < len(update.Delta.Els) {
		text = update.Delta.Els[i].Insert
		if text != "" {
			i++
		}
	}
	endOffset := len(l.Raw)
	if i < len(update.Delta.Els) {
		endCopyEl := update.Delta.Els[i]
		endOffset = endCopyEl.Copy[0]
	}
	endRow, endCol := v.GetPos(endOffset)
	if startOffset == endOffset && text == "" {
		return startRow, startCol, endRow, endCol, text, deletedText, false
	}

	deletedText = string(l.Raw[startOffset:endOffset])
	diff := endOffset - startOffset
	if diff < len(text) {
		for i := 0; i < len(text)-diff; i++ {
			l.Raw = append(l.Raw, 0)
		}
	}
	copy(l.Raw[startOffset+len(text):], l.Raw[endOffset:])
	copy(l.Raw[startOffset:startOffset+len(text)], []byte(text))
	if diff > len(text) {
		l.Raw = l.Raw[:len(l.Raw)-(diff-len(text))]
	}

	oldN := endRow - startRow

	newLines := strings.Split(text, "\n")
	for i := range newLines {
		if i < len(newLines)-1 {
			newLines[i] += "\n"
		}
	}
	newN := len(newLines) - 1
	if newN > oldN {
		for i := 0; i < newN-oldN; i++ {
			l.Lines = append(l.Lines, nil)
		}
		copy(l.Lines[startRow+newN:], l.Lines[endRow:])
	}

	for i := 0; i <= newN; i++ {
		newText := newLines[i]
		idx := startRow + i
		if newN == 0 {
			startLine := l.Lines[startRow]
			endLine := l.Lines[endRow]
			l.Lines[idx] = &Line{
				Text: string(startLine.Text[:startCol]) + newText + string(endLine.Text[endCol:]),
			}
		} else if i == 0 {
			line := l.Lines[startRow]
			l.Lines[idx] = &Line{
				Text: string(line.Text[:startCol]) + newText,
			}
		} else if i == newN {
			line := l.Lines[startRow+newN]
			l.Lines[idx] = &Line{
				Text: newText + string(line.Text[endCol:]),
			}
		} else {
			l.Lines[idx] = &Line{
				Text: newText,
			}
		}
		l.Lines[idx].length = len(l.Lines[idx].Text)
	}

	if newN < oldN {
		copy(l.Lines[startRow+newN+1:], l.Lines[endRow+1:])
		l.Lines = l.Lines[:len(l.Lines)-(oldN-newN)]
	}
	v.Offset = startOffset + len(text)
	v.Row, v.Col = v.GetPos(v.Offset)
	return startRow, startCol, endRow, endCol, text, deletedText, true
}

// GetPos gets pos
func (v *View) GetPos(offset int) (int, int) {
	row := 0
	idx := 0
	var line *Line
	for row, line = range v.LineCache.Lines {
		if idx+line.length > offset {
			return row, offset - idx
		}
		idx += line.length
	}
	return row, line.length
}

// GetOffset is
func (v *View) GetOffset(row, col int) int {
	offset := 0
	lines := v.LineCache.Lines
	for i := 0; i <= row; i++ {
		if i == row {
			offset += col
			return offset
		}
		if i >= len(lines) {
			return offset
		}
		offset += lines[i].length
	}
	return offset
}
