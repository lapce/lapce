use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;

#[derive(Eq, PartialEq, Hash, Clone)]
pub enum PanelPosition {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}

pub trait PanelProperty {
    fn position(&self) -> &PanelPosition;
    fn active(&self) -> usize;
    fn size(&self) -> (usize, f64);
}

pub struct PanelState {
    pub panels: Vec<Arc<Mutex<dyn PanelProperty>>>,
    pub shown: HashMap<PanelPosition, bool>,
}

impl PanelState {
    pub fn is_shown(&self, position: &PanelPosition) -> bool {
        *self.shown.get(position).unwrap_or(&false)
    }

    pub fn size(&self, position: &PanelPosition) -> Option<(usize, f64)> {
        let mut active_panel = None;
        let mut active = 0;
        for panel in self.panels.iter() {
            let local_panel = panel.clone();
            let panel = panel.lock();
            if panel.position() == position {
                let panel_active = panel.active();
                if panel_active > active {
                    active_panel = Some(local_panel);
                    active = panel_active;
                }
            }
        }
        active_panel.map(|p| p.lock().size())
    }

    pub fn shown_panels(
        &self,
    ) -> HashMap<PanelPosition, Option<(usize, Arc<Mutex<dyn PanelProperty>>)>> {
        let mut shown_panels = HashMap::new();
        for (postion, shown) in self.shown.iter() {
            if *shown {
                shown_panels.insert(postion.clone(), None);
            }
        }

        for panel in self.panels.iter() {
            let local_panel = panel.clone();
            let panel = panel.lock();
            let position = panel.position();
            let active = panel.active();
            if let Some(p) = shown_panels.get_mut(&position) {
                if p.is_none() {
                    *p = Some((active, local_panel));
                } else {
                    if active > p.as_ref().unwrap().0 {
                        *p = Some((active, local_panel))
                    }
                }
            }
        }
        shown_panels
    }
}
