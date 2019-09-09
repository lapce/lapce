use crate::input::{Command, KeyInput, KeyMap};
use crate::line_cache::Style;
use cairo::{FontFace, FontOptions, FontSlant, FontWeight, Matrix, ScaledFont};
use druid::shell::piet;
use piet::{CairoText, Font, FontBuilder, Text};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use syntect::highlighting::ThemeSettings;

#[derive(Clone)]
pub struct Config {
    pub font: Arc<Mutex<AppFont>>,
    pub styles: Arc<Mutex<HashMap<usize, Style>>>,
    pub theme: Arc<Mutex<ThemeSettings>>,
    pub keymaps: Arc<Mutex<KeyMap>>,
}

impl Config {
    pub fn new(font: AppFont) -> Config {
        Config {
            font: Arc::new(Mutex::new(font)),
            styles: Arc::new(Mutex::new(HashMap::new())),
            theme: Default::default(),
            keymaps: Arc::new(Mutex::new(KeyMap::new())),
        }
    }

    pub fn insert_style(&self, style_id: usize, style: Style) {
        self.styles.lock().unwrap().insert(style_id, style);
    }
}

fn scale_matrix(scale: f64) -> Matrix {
    Matrix {
        xx: scale,
        yx: 0.0,
        xy: 0.0,
        yy: scale,
        x0: 0.0,
        y0: 0.0,
    }
}

#[derive(Clone)]
pub struct AppFont {
    // font_face: FontFace,
    // font: Box<Font + Send>,
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub linespace: f64,
}

impl AppFont {
    pub fn new(family: &str, size: f64, linespace: f64) -> AppFont {
        let font_face = FontFace::toy_create("Consolas", FontSlant::Normal, FontWeight::Normal);
        let font_matrix = scale_matrix(13.0);
        let ctm = scale_matrix(1.0);
        let options = FontOptions::default();
        let scaled_font = ScaledFont::new(&font_face, &font_matrix, &ctm, &options);

        let extents = scaled_font.extents();

        let font = CairoText::new()
            .new_font_by_name("Consolas", 13.0)
            .unwrap()
            .build()
            .unwrap();

        println!("{:?} {:?}", extents, scaled_font.text_extents("W"));
        AppFont {
            // font_face,
            // font: Box::new(font) as Box<Font + Send>,
            width: scaled_font.text_extents("W").x_advance,
            ascent: extents.ascent,
            descent: extents.descent,
            linespace,
        }
    }

    pub fn lineheight(&self) -> f64 {
        self.ascent + self.descent + self.linespace
    }
}
