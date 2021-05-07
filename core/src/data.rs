use druid::{Data, Lens, WidgetId, WindowId};
use im;

#[derive(Clone, Data)]
pub struct LapceData {
    pub windows: im::HashMap<WindowId, LapceWindowData>,
}

impl LapceData {
    pub fn load() -> Self {
        let mut windows = im::HashMap::new();
        let window = LapceWindowData::new();
        windows.insert(WindowId::next(), window);
        Self { windows }
    }
}

#[derive(Clone, Data)]
pub struct LapceWindowData {
    pub tabs: im::HashMap<WidgetId, LapceTabData>,
}

impl LapceWindowData {
    pub fn new() -> Self {
        let mut tabs = im::HashMap::new();
        let tab = LapceTabData::new();
        tabs.insert(WidgetId::next(), tab);
        Self { tabs }
    }
}

#[derive(Clone, Data, Lens)]
pub struct LapceTabData {}

impl LapceTabData {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct LapceTabLens(pub WidgetId);

impl Lens<LapceWindowData, LapceTabData> for LapceTabLens {
    fn with<V, F: FnOnce(&LapceTabData) -> V>(
        &self,
        data: &LapceWindowData,
        f: F,
    ) -> V {
        let tab = data.tabs.get(&self.0).unwrap();
        f(&tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceTabData) -> V>(
        &self,
        data: &mut LapceWindowData,
        f: F,
    ) -> V {
        let mut tab = data.tabs.get_mut(&self.0).unwrap();
        f(&mut tab)
    }
}

pub struct LapceWindowLens(pub WindowId);

impl Lens<LapceData, LapceWindowData> for LapceWindowLens {
    fn with<V, F: FnOnce(&LapceWindowData) -> V>(
        &self,
        data: &LapceData,
        f: F,
    ) -> V {
        let tab = data.windows.get(&self.0).unwrap();
        f(&tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceWindowData) -> V>(
        &self,
        data: &mut LapceData,
        f: F,
    ) -> V {
        let mut tab = data.windows.get_mut(&self.0).unwrap();
        f(&mut tab)
    }
}
