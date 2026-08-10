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

use std::rc::Rc;
use std::sync::Arc;

use crate::format::{Component, ComponentType};
use crate::render;
use crate::render::hud::{Hud, HudContext};
use crate::screen::{Screen, ScreenSystem};
use crate::ui;
use crate::Game;
use parking_lot::RwLock;

pub struct DirectConnection {
    elements: Option<UIElements>,
}

impl Clone for DirectConnection {
    fn clone(&self) -> Self {
        DirectConnection { elements: None }
    }
}

struct UIElements {
    logo: ui::logo::Logo,

    _address: ui::TextBoxRef,
    _connect: ui::ButtonRef,
    _cancel: ui::ButtonRef,
}

impl DirectConnection {
    pub fn new() -> DirectConnection {
        DirectConnection { elements: None }
    }
}

impl super::Screen for DirectConnection {
    fn on_active(
        &mut self,
        _screen_sys: &ScreenSystem,
        renderer: Arc<render::Renderer>,
        ui_container: &mut ui::Container,
    ) {
        let logo = ui::logo::Logo::new(renderer.resources.clone(), ui_container);

        ui::TextBuilder::new()
            .text("Direct Connection")
            .position(0.0, -60.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);

        // Address
        let address = ui::TextBoxBuilder::new()
            .input("")
            .position(0.0, 20.0)
            .size(400.0, 40.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        ui::TextBox::make_focusable(&address, ui_container);
        ui::TextBuilder::new()
            .text("Address:")
            .position(0.0, -18.0)
            .attach(&mut *address.borrow_mut());

        let connect_error = ui::TextBuilder::new()
            .text("")
            .position(0.0, 150.0)
            .colour((255, 50, 50, 255))
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);

        let connect: Rc<dyn Fn(&Game)> = {
            let address = address.clone();
            let connect_error = connect_error.clone();
            Rc::new(move |game| {
                let target = address.borrow().input.clone();
                if target.is_empty() {
                    connect_error.borrow_mut().text = "Please enter a Server Address".into();
                    return;
                }
                game.screen_sys
                    .clone()
                    .replace_screen(Box::new(super::connecting::Connecting::new(&target)));
                let hud_context = Arc::new(RwLock::new(HudContext::new()));
                let result = game.connect_to(&target, hud_context.clone());
                game.screen_sys.clone().pop_screen();
                if let Err(error) = result {
                    game.screen_sys.clone().add_screen(Box::new(super::ServerList::new(
                        Some(Component::new(ComponentType::new(
                            &error.to_string(),
                            None,
                        ))),
                    )));
                } else {
                    game.screen_sys
                        .clone()
                        .add_screen(Box::new(Hud::new(hud_context)));
                }
            })
        };

        // Connect
        let connect_btn = ui::ButtonBuilder::new()
            .position(110.0, 100.0)
            .size(200.0, 40.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        {
            let mut connect_btn = connect_btn.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Connect")
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *connect_btn);
            connect_btn.add_text(txt);
            let connect = connect.clone();
            connect_btn.add_click_func(move |_, game| {
                (&*connect)(game);
                true
            });
        }

        // Cancel
        let cancel = ui::ButtonBuilder::new()
            .position(-110.0, 100.0)
            .size(200.0, 40.0)
            .alignment(ui::VAttach::Middle, ui::HAttach::Center)
            .create(ui_container);
        {
            let mut cancel = cancel.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Cancel")
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *cancel);
            cancel.add_text(txt);
            cancel.add_click_func(|_, game| {
                game.screen_sys
                    .clone()
                    .replace_screen(Box::new(super::ServerList::new(None)));
                true
            });
        }

        // Pressing Enter in the address box also connects
        {
            let connect = connect.clone();
            address
                .borrow_mut()
                .add_submit_func(move |_, game| (&*connect)(game));
        }

        self.elements = Some(UIElements {
            logo,
            _address: address,
            _connect: connect_btn,
            _cancel: cancel,
        });
    }

    fn on_deactive(
        &mut self,
        _screen_sys: &ScreenSystem,
        _renderer: Arc<render::Renderer>,
        _ui_container: &mut ui::Container,
    ) {
        // Clean up
        self.elements = None
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

    fn is_closable(&self) -> bool {
        true
    }

    fn clone_screen(&self) -> Box<dyn Screen> {
        Box::new(self.clone())
    }
}
