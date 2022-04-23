use unicode_segmentation::UnicodeSegmentation;//graphemes
use termion::event::Key;//Key
use termion::input::TermRead; //StdinLock
use termion::raw::{RawTerminal};//Screen
mod editor;
mod terminal;

#[derive(Default)]
pub struct Coords {
    pub latitude_pos: usize,
    pub longit_pos: usize,
}

#[derive(Default)]
pub struct Editor {
    pub index: Coords,
    pub docs: textEditor,
}

impl Editor {
    fn delete_key_press(&mut self) -> Result<(), std::io::Error> {
        let mut del_key_pressed = Screen::read_delete_key()?;
        match del_key_pressed {
            Key::Delete => self.docs.delete(&self.index),
            empty => (),
        }
        Ok(())
    }
}


#[derive(Default)]
pub struct textEditor {
    line: Vec<LineText>,//vector for multiple lines
}

impl textEditor {

    pub fn len(&self) -> usize {
        return self.line.len();//return len as reference
    }

    pub fn delete(&mut self, current: &Coords) {
        if current.longit_pos >= self.len() {//recursion for delete
            return;
        } else {
            let mut idex_line = self.line.get_mut(current.longit_pos).unwrap();
            idex_line.delete(current.latitude_pos);//can press and hold delete keyword here
        }
    }
}


#[derive(Default)]
pub struct LineText {//in reference to entire text paragaph
    text: String,
    length: usize,
}

impl LineText {
    pub fn len(&self) -> usize {
        return self.length;//gets length as reference
    }
    pub fn delete(&mut self, at: usize) {
        if at >= self.len() {
            return;//no deletion necessary
        } else {//deletion necessary
            let mut original: String = self.text[..].graphemes(true).take(at).collect();//using graphemes for word boundaries
            let deleted: String = self.text[..].graphemes(true).skip(at + 1).collect();
            original.push_str(&deleted);
            self.text = original;
        }
        self.length = self.text[..].graphemes(true).count();

    }
}


pub struct Screen;
impl Screen {
pub fn read_delete_key() -> Result<Key, std::io::Error> {//read delete keyword
        loop {//infinite loop
            if  let Some(mut new_index) = std::io::stdin().lock().keys().next() {//some new value
                return new_index;
            }
        }
	}

}
