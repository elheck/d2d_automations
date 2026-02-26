use eframe::egui;

pub struct OutputWindow<'a> {
    pub title: &'a str,
    pub content: &'a mut String,
    pub show: &'a mut bool,
    pub default_extension: &'a str,
}

impl<'a> OutputWindow<'a> {
    pub fn new(
        title: &'a str,
        content: &'a mut String,
        show: &'a mut bool,
        default_extension: &'a str,
    ) -> Self {
        Self {
            title,
            content,
            show,
            default_extension,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        egui::Window::new(self.title)
            .default_size([800.0, 600.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(self.content)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        *self.show = false;
                    }
                    #[cfg(not(target_os = "android"))]
                    if ui.button("Save to File").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("output.{}", self.default_extension))
                            .add_filter(
                                if self.default_extension == "csv" {
                                    "CSV Files"
                                } else {
                                    "Text Files"
                                },
                                &[self.default_extension],
                            )
                            .save_file()
                        {
                            if let Err(e) = std::fs::write(&path, &self.content) {
                                *self.content = format!("Error saving file: {e}");
                            }
                        }
                    }
                });
            });
    }
}
