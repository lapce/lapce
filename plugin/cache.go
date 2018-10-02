package plugin

import "github.com/crane-editor/crane/log"

// Cache is
type Cache struct {
	lineOffsets []int
	content     []byte
	offset      int
}

// SetContent sets
func (c *Cache) SetContent(content []byte) {
	log.Infoln("cache set content")
	c.content = content
	c.resetLineOffsets()
}

// GetOffset sets
func (c *Cache) GetOffset() int {
	return c.offset
}

// GetContent sets
func (c *Cache) GetContent() []byte {
	return c.content
}

// ApplyUpdate applys update
func (c *Cache) ApplyUpdate(update *Update) {
	newContent := make([]byte, update.NewLen)
	i := 0
	for _, el := range update.Delta.Els {
		n := 0
		if len(el.Copy) > 0 {
			n = copy(newContent[i:], c.content[el.Copy[0]:el.Copy[1]])
			if i == 0 {
				c.offset = i + n
			}
		} else {
			n = copy(newContent[i:], []byte(el.Insert))
			c.offset = i + n
		}
		i += n
	}
	c.content = newContent
	c.resetLineOffsets()
}

func (c *Cache) resetLineOffsets() {
	c.lineOffsets = []int{}
	for i, char := range c.content {
		if char == '\n' {
			c.lineOffsets = append(c.lineOffsets, i)
		}
	}
}

// GetChunk gets
func (c *Cache) GetChunk(startOffset, endOffset int) []byte {
	return c.content[startOffset:endOffset]
}

// GetLine gets
func (c *Cache) GetLine(row int) []byte {
	start := 0
	if row > 0 && row <= len(c.lineOffsets) {
		start = c.lineOffsets[row-1] + 1
	}
	end := 0
	if row <= len(c.lineOffsets) {
		end = c.lineOffsets[row]
	}
	return c.content[start:end]
}

// OffsetToPos returns
func (c *Cache) OffsetToPos(offset int) (row int, col int) {
	log.Infoln("offset is", offset)

	lastLineOffset := 0
loop:
	for _, lineOffset := range c.lineOffsets {
		log.Infoln("lineOffset is", lineOffset)
		if offset > lineOffset {
			row++
			lastLineOffset = lineOffset
			continue loop
		}
		col = offset - lastLineOffset - 1
		log.Infoln("pos is", row, col)
		return
	}
	log.Infoln("pos is", row, col)
	return
}

// PosToOffset returns
func (c *Cache) PosToOffset(row, col int) int {
	log.Infoln("pos is", row, col)
	offset := 0
	if row > len(c.lineOffsets) {
		offset = len(c.content)
	} else {
		if row-1 >= 0 {
			offset = c.lineOffsets[row-1] + 1
		}
	}

	offset += col
	log.Infoln("offset is", offset)
	return offset
}
