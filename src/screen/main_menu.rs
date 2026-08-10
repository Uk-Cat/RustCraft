// Copyright 2016 Matthew Collins
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;

use crate::render;
use crate::screen::{Screen, ScreenSystem, ServerList};
use crate::ui;

pub struct MainMenu {
    elements: Option<UIElements>,
}

impl Clone for MainMenu {
    fn clone(&self) -> Self {
        MainMenu::new()
    }
}

struct UIElements {
    logo: ui::logo::Logo,

    _singleplayer_btn: ui::ButtonRef,
    _multiplayer_btn: ui::ButtonRef,
    _options_btn: ui::ButtonRef,
    _quit_btn: ui::ButtonRef,
}

impl MainMenu {
    pub fn new() -> MainMenu {
        MainMenu { elements: None }
    }
}

impl super::Screen for MainMenu {
    fn on_active(
        &mut self,
        _screen_sys: &ScreenSystem,
        renderer: Arc<render::Renderer>,
        ui_container: &mut ui::Container,
    ) {
        // Title
        let logo = ui::logo::Logo::new_sized(renderer.resources.clone(), ui_container, 55.0, 140.0);

        // Singleplayer, centered on the middle of the screen
        let singleplayer = ui::ButtonBuilder::new()
            .position(0.0, 0.0)
            .size(560.0, 55.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        {
            let mut singleplayer = singleplayer.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Singleplayer")
                .scale_x(1.35)
                .scale_y(1.35)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *singleplayer);
            singleplayer.add_text(txt);
        }

        // Multiplayer, below singleplayer
        let multiplayer = ui::ButtonBuilder::new()
            .position(0.0, 62.0)
            .size(560.0, 55.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        {
            let mut multiplayer = multiplayer.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Multiplayer")
                .scale_x(1.35)
                .scale_y(1.35)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *multiplayer);
            multiplayer.add_text(txt);
            multiplayer.add_click_func(|_, game| {
                game.screen_sys
                    .clone()
                    .replace_screen(Box::new(ServerList::new(None)));
                true
            });
        }

        // Options and Quit Game side by side, combined width matches the
        // singleplayer/multiplayer buttons
        let options = ui::ButtonBuilder::new()
            .position(-142.0, 140.0)
            .size(276.0, 55.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        {
            let mut options = options.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Options...")
                .scale_x(1.35)
                .scale_y(1.35)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *options);
            options.add_text(txt);
            options.add_click_func(|_, game| {
                game.screen_sys
                    .clone()
                    .add_screen(Box::new(super::SettingsMenu::new(
                        game.settings.clone(),
                        false,
                    )));
                true
            });
        }

        let quit = ui::ButtonBuilder::new()
            .position(142.0, 140.0)
            .size(276.0, 55.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        {
            let mut quit = quit.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Quit Game")
                .scale_x(1.35)
                .scale_y(1.35)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *quit);
            quit.add_text(txt);
            quit.add_click_func(|_, game| {
                game.set_should_close();
                true
            });
        }

        self.elements = Some(UIElements {
            logo,

            _singleplayer_btn: singleplayer,
            _multiplayer_btn: multiplayer,
            _options_btn: options,
            _quit_btn: quit,
        });
    }

    fn on_deactive(
        &mut self,
        _screen_sys: &ScreenSystem,
        _renderer: Arc<render::Renderer>,
        _ui_container: &mut ui::Container,
    ) {
        self.elements = None;
    }

    fn tick(
        &mut self,
        _screen_sys: &ScreenSystem,
        renderer: Arc<render::Renderer>,
        _ui_container: &mut ui::Container,
        _delta: f64,
    ) {
        let elements = self.elements.as_mut().unwrap();
        elements.logo.tick(renderer);
    }

    fn clone_screen(&self) -> Box<dyn Screen> {
        Box::new(self.clone())
    }
}
