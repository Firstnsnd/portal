use eframe::egui;

struct TestApp {
    show_child: bool,
}

impl eframe::App for TestApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Child viewport
        if self.show_child {
            let vp_id = egui::ViewportId::from_hash_of("child_window");
            let builder = egui::ViewportBuilder::default()
                .with_title("Child Window")
                .with_inner_size([400.0, 300.0]);
            ctx.show_viewport_immediate(vp_id, builder, |ctx, class| {
                eprintln!("viewport callback called");
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Hello from child viewport!");
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.show_child = false;
                }
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Main Window");
            if ui.button("Open Child Window").clicked() {
                self.show_child = true;
                eprintln!("show_child set to true");
            }
            ui.label(format!("show_child = {}", self.show_child));
            ui.label(format!("embed_viewports = {}", ctx.embed_viewports()));
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_title("Viewport Test"),
        ..Default::default()
    };
    eframe::run_native(
        "viewport_test",
        options,
        Box::new(|_cc| Ok(Box::new(TestApp { show_child: false }))),
    )
}
