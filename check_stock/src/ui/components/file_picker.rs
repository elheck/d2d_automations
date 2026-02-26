use eframe::egui;

pub struct FilePicker<'a> {
    pub label: &'a str,
    pub path: &'a mut String,
    pub filter_name: Option<&'a str>,
    pub extensions: &'a [&'a str],
}

impl<'a> FilePicker<'a> {
    pub fn new(label: &'a str, path: &'a mut String) -> Self {
        Self {
            label,
            path,
            filter_name: None,
            extensions: &[],
        }
    }

    pub fn with_filter(mut self, name: &'a str, extensions: &'a [&'a str]) -> Self {
        self.filter_name = Some(name);
        self.extensions = extensions;
        self
    }

    /// Shows the file picker widget. Returns `true` if a file was just selected.
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        #[cfg(not(target_os = "android"))]
        {
            self.show_desktop(ui)
        }
        #[cfg(target_os = "android")]
        {
            self.show_android(ui)
        }
    }

    /// Desktop implementation: native OS file dialog via rfd.
    #[cfg(not(target_os = "android"))]
    fn show_desktop(&mut self, ui: &mut egui::Ui) -> bool {
        let mut picked = false;
        ui.horizontal(|ui| {
            ui.label(self.label);
            if ui.button("Browse").clicked() {
                let mut dialog = rfd::FileDialog::new();

                if let Some(name) = self.filter_name {
                    dialog = dialog.add_filter(name, self.extensions);
                }

                if let Some(path) = dialog.pick_file() {
                    *self.path = path.display().to_string();
                    picked = true;
                }
            }
            ui.text_edit_singleline(self.path);
        });
        picked
    }

    /// Android implementation: text field only (no native file dialog available).
    #[cfg(target_os = "android")]
    fn show_android(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(self.label);
            changed = ui.text_edit_singleline(self.path).changed();
        });
        changed
    }
}
