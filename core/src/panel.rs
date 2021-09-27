use std::{collections::HashMap, sync::Arc};

use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Size, Widget, WidgetId, WindowId,
};
use parking_lot::Mutex;

use crate::{
    command::LapceUICommand, command::LAPCE_UI_COMMAND, explorer::FileExplorerState,
    outline::OutlineState,
};

pub enum PanelResizePosition {
    Left,
    LeftSplit,
}

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
    fn widget_id(&self) -> WidgetId;
    fn position(&self) -> &PanelPosition;
    fn active(&self) -> usize;
    fn size(&self) -> (f64, f64);
}

pub struct PanelState {
    window_id: WindowId,
    tab_id: WidgetId,
    pub panels: HashMap<WidgetId, Arc<Mutex<dyn PanelProperty>>>,
    pub shown: HashMap<PanelPosition, bool>,
    pub widgets: HashMap<PanelPosition, WidgetId>,
}

impl PanelState {
    pub fn new(window_id: WindowId, tab_id: WidgetId) -> Self {
        let panels = HashMap::new();
        let mut shown = HashMap::new();
        shown.insert(PanelPosition::LeftTop, true);
        shown.insert(PanelPosition::LeftBottom, true);
        shown.insert(PanelPosition::BottomLeft, true);
        shown.insert(PanelPosition::RightTop, true);

        let mut widgets = HashMap::new();
        widgets.insert(PanelPosition::LeftTop, WidgetId::next());
        widgets.insert(PanelPosition::LeftBottom, WidgetId::next());
        widgets.insert(PanelPosition::BottomLeft, WidgetId::next());
        widgets.insert(PanelPosition::BottomRight, WidgetId::next());
        widgets.insert(PanelPosition::RightTop, WidgetId::next());
        widgets.insert(PanelPosition::RightBottom, WidgetId::next());
        Self {
            window_id,
            tab_id,
            panels,
            shown,
            widgets,
        }
    }

    pub fn is_shown(&self, position: &PanelPosition) -> bool {
        *self.shown.get(position).unwrap_or(&false)
    }

    pub fn add(
        &mut self,
        widget_id: WidgetId,
        panel: Arc<Mutex<dyn PanelProperty>>,
    ) {
        self.panels.insert(widget_id, panel);
    }

    pub fn widget_id(&self, position: &PanelPosition) -> WidgetId {
        self.widgets.get(position).unwrap().clone()
    }

    pub fn size(&self, position: &PanelPosition) -> Option<(f64, f64)> {
        let mut active_panel = None;
        let mut active = 0;
        for (_, panel) in self.panels.iter() {
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

    pub fn get(
        &self,
        position: &PanelPosition,
    ) -> Option<Arc<Mutex<dyn PanelProperty>>> {
        let mut active_panel = None;
        for (_, panel) in self.panels.iter() {
            let local_panel = panel.clone();
            let (current_position, active) = {
                let panel = panel.lock();
                let position = panel.position().clone();
                let active = panel.active();
                (position, active)
            };
            if &current_position == position {
                if active_panel.is_none() {
                    active_panel = Some(local_panel);
                } else {
                    if active > active_panel.as_ref().unwrap().lock().active() {
                        active_panel = Some(local_panel)
                    }
                }
            }
        }
        active_panel
    }

    pub fn shown_panels(
        &self,
    ) -> HashMap<PanelPosition, Arc<Mutex<dyn PanelProperty>>> {
        let mut shown_panels = HashMap::new();
        for (postion, shown) in self.shown.iter() {
            if *shown {
                shown_panels.insert(postion.clone(), None);
            }
        }

        for (_, panel) in self.panels.iter() {
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
    widget_id: WidgetId,
    window_id: WindowId,
    tab_id: WidgetId,
    position: PanelPosition,
}

impl LapcePanel {
    pub fn new(
        widget_id: WidgetId,
        window_id: WindowId,
        tab_id: WidgetId,
        position: PanelPosition,
    ) -> Self {
        Self {
            widget_id,
            window_id,
            tab_id,
            position,
        }
    }
}
