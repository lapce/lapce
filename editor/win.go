package editor

// Frame is
type Frame struct {
	vertical bool
	width    int
	height   int
	x        int
	y        int
	f1       *Frame
	f2       *Frame
	parent   *Frame
	vTop     *Frame
	hTop     *Frame
}

func (f *Frame) split(vertical bool) {
	if f.f1 != nil || f.f2 != nil {
		// alreday split can't split again
		return
	}
	if f.vTop == nil && vertical {
		f.vTop = f
	}
	if f.hTop == nil && !vertical {
		f.hTop = f
	}
	f.vertical = vertical
	f.f1 = &Frame{
		parent: f,
		vTop:   f.vTop,
		hTop:   f.hTop,
	}
	f.f2 = &Frame{
		parent: f,
		vTop:   f.vTop,
		hTop:   f.hTop,
	}
	f.equal(vertical)
}

func (f *Frame) equal(vertical bool) {
	top := f.vTop
	if !vertical {
		top = f.hTop
	}

	value := top.width
	if !vertical {
		value = top.height
	}
	singleValue := value / top.countSplits(vertical)
	top.setSize(vertical, singleValue)
	top.setPos(top.x, top.y)
}

func (f *Frame) setPos(x, y int) {
	if f.f1 == nil {
		f.f2.setPos(x, y)
		return
	}
	if f.f2 == nil {
		f.f1.setPos(x, y)
		return
	}
	if f.vertical {
		f.f1.setPos(x, y)
		f.f2.setPos(x+f.f1.width, y)
		return
	}
	f.f1.setPos(x, y)
	f.f2.setPos(x, y+f.f1.height)
}

func (f *Frame) setSize(vertical bool, singleValue int) {
	if !f.hasSplit() {
		if vertical {
			f.width = singleValue
		} else {
			f.height = singleValue
		}
		// set value
		return
	}
	if f.f1 == nil {
		f.f2.setSize(vertical, singleValue)
		return
	}
	if f.f2 == nil {
		f.f1.setSize(vertical, singleValue)
		return
	}
	if f.vertical == vertical {
		f.f1.setSize(vertical, singleValue)
		f.f2.setSize(vertical, singleValue)
		return
	}
	n1 := 0
	n2 := 0
	n1 = f.f1.countSplits(vertical)
	n2 = f.f2.countSplits(vertical)
	if n1 == n2 {
		f.f1.setSize(vertical, singleValue)
		f.f2.setSize(vertical, singleValue)
		return
	}
	newsingleValue := singleValue * Max(n1, n2) / Min(n1, n2)
	if n1 > n2 {
		f.f1.setSize(vertical, singleValue)
		f.f2.setSize(vertical, newsingleValue)
		return
	}
	f.f1.setSize(vertical, newsingleValue)
	f.f2.setSize(vertical, singleValue)
}

func (f *Frame) close() {
	if f.f1 != nil || f.f2 != nil {
		// can't close frame that has children
		return
	}
	if f.parent.f1 == f {
		f.parent.f1 = nil
	} else {
		f.parent.f2 = nil
	}
	if !f.parent.hasSplit() {
		f.parent.close()
	} else {
		f.parent.equal(f.parent.vertical)
	}
}

func (f *Frame) countSplits(vertical bool) int {
	if !f.hasSplit() {
		return 1
	}
	n1 := 0
	n2 := 0
	if f.f1 != nil {
		n1 = f.f1.countSplits(vertical)
	}
	if f.f2 != nil {
		n2 = f.f2.countSplits(vertical)
	}
	if f.vertical == vertical {
		return n1 + n2
	}
	return Max(n1, n2)
}

func (f *Frame) hasSplit() bool {
	return f.f1 != nil || f.f2 != nil
}
