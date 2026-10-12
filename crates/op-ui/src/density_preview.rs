//! Headless style artifact using the real theme, fonts and widgets, not a window screenshot.
use egui::{Rect, pos2, vec2};
use op_application::prefs::UiDensity;

pub fn render(folder: &std::path::Path) {
    let gpu = op_render::Gpu::headless().expect("GPU for style artifact");
    let mut readback = op_render::Renderer::new(gpu.clone());
    for (density, name) in [
        (UiDensity::Compact, "compact"),
        (UiDensity::Comfortable, "comfortable"),
        (UiDensity::Spacious, "spacious"),
    ] {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, "inter");
        crate::theme::apply(&ctx, 1.0, density);
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0,0.0),vec2(1000.0,650.0))), ..Default::default() }, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.heading(format!("OpenPremier · {name}"));
                ui.label("Validación de estilo: controles reales, no captura de la ventana de edición.");
                ui.add_space(12.0);
                ui.horizontal(|ui| { for label in ["Importar", "Acciones rápidas", "Exportar", "Preferencias"] { let _ = ui.button(label); } });
                ui.separator();
                ui.columns(2, |columns| {
                    let ui = &mut columns[0];
                    crate::widgets::section(ui, egui::Id::new("motion"), "Controles de efectos · Movimiento", true);
                    egui::Grid::new("values").spacing([18.0,12.0]).show(ui, |ui| {
                        for (label,mut value) in [("Posición X",960.0),("Posición Y",540.0),("Escala",100.0),("Opacidad",100.0)] {
                            ui.label(label); ui.add(egui::DragValue::new(&mut value)); ui.end_row();
                        }
                    });
                    ui.add_space(12.0);
                    crate::widgets::section(ui, egui::Id::new("media"), "Proyecto · Archivos", true);
                    for item in ["Entrevista · Cámara 1", "Música · Ambiente", "Título de apertura", "Subtítulos"] {
                        let response = ui.add_sized([ui.available_width(),crate::theme::row_height(ui)], egui::Button::new(item));
                        assert!(response.rect.height() >= crate::theme::row_height(ui));
                    }
                    let ui = &mut columns[1];
                    crate::widgets::section(ui, egui::Id::new("preferences"), "Preferencias · Reproducción y exportación", true);
                    let mut enabled = true;
                    ui.checkbox(&mut enabled,"Crear proxies al importar video");
                    egui::ComboBox::from_id_salt("resolution").selected_text("720p").show_ui(ui, |ui| { ui.label("540p / 720p / 1080p"); });
                    ui.label("Motor de renderizado");
                    egui::ComboBox::from_id_salt("renderer").selected_text("GPU nativa (wgpu)").show_ui(ui, |ui| { ui.label("Software / fframes CPU"); });
                    ui.add_space(12.0);
                    crate::widgets::section(ui, egui::Id::new("transport"), "Monitor · Transporte", true);
                    ui.horizontal(|ui| { for icon in [crate::icons::Icon::Play,crate::icons::Icon::Plus,crate::icons::Icon::Wrench] { crate::icons::button(ui,icon,20.0,false,""); } });
                    crate::widgets::dim_label(ui,"Los paneles y diseños guardados se conservan.");
                });
            });
        });
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = op_render::Tex::new(
            &gpu.device,
            1000,
            650,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            "density-preview",
        );
        let mut renderer =
            egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::PREDICTABLE);
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let jobs = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [1000, 650],
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let commands =
            renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &texture.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &jobs, &screen);
        }
        gpu.queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        output.textures_delta.clear();
        let rgba = readback.read_texture(&texture, 4).unwrap();
        std::fs::create_dir_all(folder).unwrap();
        image::save_buffer(
            folder.join(format!("density-{name}.png")),
            &rgba,
            1000,
            650,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
