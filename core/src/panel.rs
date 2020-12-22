use std::{collections::HashMap, sync::Arc};

use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Size, Widget, WidgetId, WindowId,
};
use parking_lot::Mutex;

use crate::{explorer::FileExplorerState, state::LapceUIState};

#[derive(Eq, PartialEq, Hash, Clone)]
pub enum PanelPosition {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}

pub trait PanelProperty: Send {
    fn position(&self) -> &PanelPosition;
    fn active(&self) -> usize;
    fn size(&self) -> (f64, f64);
}

pub struct PanelState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub panels: Vec<Arc<Mutex<Box<dyn PanelProperty>>>>,
    pub shown: HashMap<PanelPosition, bool>,
}

impl PanelState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> Self {
        let file_exploer = Arc::new(Mutex::new(Box::new(FileExplorerState::new(
            window_id, tab_id,
        ))
            as Box<dyn PanelProperty>));
        let mut panels = Vec::new();
        panels.push(file_exploer);
        let mut shown = HashMap::new();
        shown.insert(PanelPosition::LeftTop, true);
        Self {
            window_id,
            tab_id,
            panels,
            shown,
        }
    }
    pub fn is_shown(&self, position: &PanelPosition) -> bool {
        *self.shown.get(position).unwrap_or(&false)
    }

    pub fn size(&self, position: &PanelPosition) -> Option<(f64, f64)> {
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
    ) -> HashMap<PanelPosition, Arc<Mutex<Box<dyn PanelProperty>>>> {
        let mut shown_panels = HashMap::new();
        for (postion, shown) in self.shown.iter() {
            if *shown {
                shown_panels.insert(postion.clone(), None);
            }
        }

        for panel in self.panels.iter() {
            let local_panel = panel.clone();
            let (position, active) = {
                let panel = panel.lock();
                let position = panel.position().clone();
                let active = panel.active();
                (position, active)
            };
            if let Some(p) = shown_panels.get_mut(&position) {
                if p.is_none() {
                    *p = Some(local_panel);
                } else {
                    if active > p.as_ref().unwrap().lock().active() {
                        *p = Some(local_panel)
                    }
                }
            }
        }
        shown_panels
            .iter()
            .filter_map(|(p, panel)| Some((p.clone(), panel.clone()?)))
            .collect()
    }
}

pub struct LapcePanel {
    window_id: WindowId,
    tab_id: WidgetId,
    position: PanelPosition,
}

impl LapcePanel {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        position: PanelPosition,
    ) -> Self {
        Self {
            window_id,
            tab_id,
            position,
        }
    }
}

impl Widget<LapceUIState> for LapcePanel {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceUIState,
        env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceUIState,
        env: &Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceUIState,
        data: &LapceUIState,
        env: &druid::Env,
    ) {
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceUIState,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceUIState, env: &Env) {}
}
