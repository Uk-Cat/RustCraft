use crate::render;
use crate::resources;
use crate::ui;
use image::GenericImageView;
use instant::Instant;
use parking_lot::RwLock;
use rand::seq::SliceRandom;
use std::f64::consts;
use std::io::Read;
use std::sync::Arc;

#[derive(Clone)]
pub struct Logo {
    _shadow: ui::BatchRef,
    _layer0: ui::BatchRef,

    text: ui::TextRef,
    text_base_scale: f64,
    text_orig_x: f64,
    text_index: isize,
    text_strings: Vec<String>,
    started: Instant,
}

impl Logo {
    pub fn new(
        resources: Arc<RwLock<resources::Manager>>,
        ui_container: &mut ui::Container,
    ) -> Logo {
        Logo::new_sized(resources, ui_container, 40.0, 128.0)
    }

    pub fn new_sized(
        resources: Arc<RwLock<resources::Manager>>,
        ui_container: &mut ui::Container,
        y: f64,
        height: f64,
    ) -> Logo {
        let shadow_batch = ui::BatchBuilder::new()
            .position(0.0, y)
            .size(100.0, 100.0)
            .alignment(ui::VAttach::Top, ui::HAttach::Center)
            .create(ui_container);
        let layer0 = ui::BatchBuilder::new()
            .position(0.0, y)
            .size(100.0, 100.0)
            .draw_index(1)
            .alignment(ui::VAttach::Top, ui::HAttach::Center)
            .create(ui_container);

        // The title is loaded as the "rustcraft-dynamic:gui/title" texture,
        // registered at startup in main.rs from resources/assets/rustcraft/textures/gui/title.png.
        // Size it using the image's aspect ratio.
        let img = image::load_from_memory(include_bytes!("../../resources/assets/rustcraft/textures/gui/title.png")).unwrap();
        let (img_w, img_h) = img.dimensions();
        let width = height * (img_w as f64) / (img_h as f64);

        ui::ImageBuilder::new()
            .texture("rustcraft-dynamic:gui/title")
            .position(2.0, 4.0)
            .size(width, height)
            .colour((0, 0, 0, 100))
            .attach(&mut *shadow_batch.borrow_mut());

        ui::ImageBuilder::new()
            .texture("rustcraft-dynamic:gui/title")
            .position(0.0, 0.0)
            .size(width, height)
            .attach(&mut *layer0.borrow_mut());

        shadow_batch.borrow_mut().width = width;
        shadow_batch.borrow_mut().height = height;
        layer0.borrow_mut().width = width;
        layer0.borrow_mut().height = height;

        let mut text_strings = vec![];
        {
            let res = resources.read();
            let mut splashes = res.open_all("minecraft", "texts/splashes.txt");
            for file in &mut splashes {
                let mut texts = String::new();
                file.read_to_string(&mut texts).unwrap();
                for line in texts.lines() {
                    text_strings.push(line.to_owned().replace('\r', ""));
                }
            }
            let mut r: rand_pcg::Pcg32 =
                rand::SeedableRng::from_seed([45, 0, 0, 0, 64, 0, 0, 0, 32, 0, 0, 0, 12, 0, 0, 0]);
            text_strings.shuffle(&mut r);
        }

        let txt = ui::TextBuilder::new()
            .text("")
            .position(0.0, -8.0)
            .colour((255, 255, 0, 255))
            .alignment(ui::VAttach::Bottom, ui::HAttach::Right)
            .draw_index(1)
            .create(&mut *layer0.borrow_mut());

        let width = txt.borrow().width;
        let mut text_base_scale = 300.0 / width;
        if text_base_scale > 1.2 {
            text_base_scale = 1.2;
        }
        txt.borrow_mut().x = (-width / 2.0) * text_base_scale;
        let text_orig_x = txt.borrow().x;

        Logo {
            _shadow: shadow_batch,
            _layer0: layer0,
            text: txt,
            text_base_scale,
            text_orig_x,
            text_index: -1,
            text_strings,
            started: Instant::now(),
        }
    }

    pub fn tick(&mut self, renderer: Arc<render::Renderer>) {
        let now = Instant::now().duration_since(self.started);

        // Splash text
        let text_index = (now.as_secs() / 15) as isize % self.text_strings.len() as isize;
        let mut text = self.text.borrow_mut();
        if self.text_index != text_index {
            self.text_index = text_index;
            text.text
                .clone_from(&self.text_strings[text_index as usize]);
            let width = (renderer.ui.lock().size_of_string(&text.text) + 2.0) * text.scale_x;
            self.text_base_scale = 300.0 / width;
            if self.text_base_scale > 1.2 {
                self.text_base_scale = 1.2;
            }
            text.x = (-width / 2.0) * self.text_base_scale;
            self.text_orig_x = text.x;
        }

        let timer = now.subsec_nanos() as f64 / 1000000000.0;
        let mut offset = timer / 0.5;
        if offset > 1.0 {
            offset = 2.0 - offset;
        }
        offset = ((offset * consts::PI).cos() + 1.0) / 2.0;
        text.scale_x = (0.9 + (offset / 4.0)) * self.text_base_scale;
        text.scale_y = (0.9 + (offset / 4.0)) * self.text_base_scale;
        let scale = text.scale_x;
        text.x = self.text_orig_x * scale * self.text_base_scale;
    }
}
