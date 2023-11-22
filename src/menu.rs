pub struct Menu {
    pub playing: bool,
    pub status: String,
}

impl ksni::Tray for Menu {
    fn icon_name(&self) -> String {
        "help-about".into()
    }
    fn title(&self) -> String {
        self.status.clone()
    }
    // NOTE: On some system trays, `id` is a required property to avoid unexpected behaviors
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.playing = !self.playing;
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![StandardItem {
            label: "Exit".into(),
            icon_name: "application-exit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        }
        .into()]
    }
}
