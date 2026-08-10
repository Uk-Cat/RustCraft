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

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

use crate::format;
use crate::format::{Component, ComponentType};
use crate::paths;
use crate::protocol;
use crate::render;
use crate::ui;

use crate::render::hud::{Hud, HudContext};
use crate::render::Renderer;
use crate::screen::{Screen, ScreenSystem};
use crate::ui::Container;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use crossbeam_channel::unbounded;
use crossbeam_channel::{Receiver, TryRecvError};
use instant::Duration;
use parking_lot::RwLock;
use rand::Rng;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct ServerList {
    elements: Option<UIElements>,
    disconnect_reason: Option<Component>,

    needs_reload: Rc<RefCell<bool>>,
    selected: Rc<RefCell<Option<(usize, String, String)>>>,
}

impl Clone for ServerList {
    fn clone(&self) -> Self {
        ServerList {
            elements: None,
            disconnect_reason: self.disconnect_reason.clone(),
            needs_reload: Rc::new(RefCell::new(false)),
            selected: Rc::new(RefCell::new(None)),
        }
    }
}

struct UIElements {
    logo: ui::logo::Logo,
    servers: Vec<Server>,
    selected: Rc<RefCell<Option<(usize, String, String)>>>,

    _join_btn: ui::ButtonRef,
    _direct_btn: ui::ButtonRef,
    _add_btn: ui::ButtonRef,
    _edit_btn: ui::ButtonRef,
    _delete_btn: ui::ButtonRef,
    _refresh_btn: ui::ButtonRef,
    _back_btn: ui::ButtonRef,
    _options_btn: ui::ButtonRef,
    _disclaimer: ui::TextRef,

    _disconnected: Option<ui::ImageRef>,
}

struct Server {
    back: ui::ImageRef,
    offset: f64,
    y: f64,
    hovered: Rc<RefCell<bool>>,

    motd: ui::FormattedRef,
    ping: ui::ImageRef,
    players: ui::TextRef,
    version: ui::FormattedRef,

    icon: ui::ImageRef,
    icon_texture: Option<String>,

    done_ping: bool,
    recv: Receiver<PingInfo>,
}

struct PingInfo {
    motd: format::Component,
    ping: Duration,
    exists: bool,
    online: i32,
    max: i32,
    protocol_version: i32,
    protocol_name: String,
    forge_mods: Vec<crate::protocol::forge::ForgeMod>,
    favicon: Option<image::DynamicImage>,
}

impl Server {
    fn update_position(&mut self) {
        if self.offset < 0.0 {
            self.y = self.offset * 200.0;
        } else {
            self.y = self.offset * 100.0;
        }
    }
}

impl ServerList {
    pub fn new(disconnect_reason: Option<Component>) -> ServerList {
        ServerList {
            elements: None,
            disconnect_reason,
            needs_reload: Rc::new(RefCell::new(false)),
            selected: Rc::new(RefCell::new(None)),
        }
    }

    fn reload_server_list(
        &mut self,
        renderer: Arc<render::Renderer>,
        ui_container: &mut ui::Container,
    ) {
        let elements = self.elements.as_mut().unwrap();
        *self.needs_reload.borrow_mut() = false;
        *self.selected.borrow_mut() = None;
        {
            // Clean up previous list icons.
            let mut tex = renderer.get_textures_ref().write();
            for server in &mut elements.servers {
                if let Some(ref icon) = server.icon_texture {
                    tex.remove_dynamic(icon);
                }
            }
        }
        elements.servers.clear();

        let file = match fs::File::open(paths::get_data_dir().join("servers.json")) {
            Ok(val) => val,
            Err(_) => return,
        };
        let servers_info: serde_json::Value = serde_json::from_reader(file).unwrap();
        let servers = servers_info.get("servers").unwrap().as_array().unwrap();
        let mut offset = 0.0;
        let selected = elements.selected.clone();

        for (index, svr) in servers.iter().enumerate() {
            let name = svr.get("name").unwrap().as_str().unwrap().to_owned();
            let address = svr.get("address").unwrap().as_str().unwrap().to_owned();

            // Everything is attached to this
            let back = ui::ImageBuilder::new()
                .texture("rustcraft:solid")
                .position(0.0, offset * 100.0)
                .size(700.0, 100.0)
                .colour((0, 0, 0, 100))
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .create(ui_container);

            let (send, recv) = unbounded();
            let hovered = Rc::new(RefCell::new(false));
            // Make whole entry interactable
            {
                let mut backr = back.borrow_mut();
                let hov = hovered.clone();
                backr.add_hover_func(move |_, over, _| {
                    *hov.borrow_mut() = over;
                    false
                });
                let sel = selected.clone();
                let sname = name.clone();
                let saddr = address.clone();
                backr.add_click_func(move |_, _| {
                    *sel.borrow_mut() = Some((index, sname.clone(), saddr.clone()));
                    true
                });
            }

            // Server name
            ui::TextBuilder::new()
                .text(name.clone())
                .position(100.0, 5.0)
                .attach(&mut *back.borrow_mut());

            // Server icon
            let icon = ui::ImageBuilder::new()
                .texture("misc/unknown_server")
                .position(5.0, 5.0)
                .size(90.0, 90.0)
                .attach(&mut *back.borrow_mut());

            // Ping indicator
            let ping = ui::ImageBuilder::new()
                .texture("gui/icons")
                .position(5.0, 5.0)
                .size(20.0, 16.0)
                .texture_coords((0.0, 56.0, 10.0, 8.0))
                .alignment(ui::VAttach::Top, ui::HAttach::Right)
                .attach(&mut *back.borrow_mut());

            // Player count
            let players = ui::TextBuilder::new()
                .text("???")
                .position(30.0, 5.0)
                .alignment(ui::VAttach::Top, ui::HAttach::Right)
                .attach(&mut *back.borrow_mut());

            // Server's message of the day
            let motd = ui::FormattedBuilder::new()
                .text(Component::new(ComponentType::new("Connecting...", None)))
                .position(100.0, 23.0)
                .max_width(700.0 - (90.0 + 10.0 + 5.0))
                .attach(&mut *back.borrow_mut());

            // Version information
            let version = ui::FormattedBuilder::new()
                .text(Component::new(ComponentType::new("", None)))
                .position(100.0, 5.0)
                .max_width(700.0 - (90.0 + 10.0 + 5.0))
                .alignment(ui::VAttach::Bottom, ui::HAttach::Left)
                .attach(&mut *back.borrow_mut());

            // Delete entry button
            let delete_entry = ui::ButtonBuilder::new()
                .position(0.0, 0.0)
                .size(25.0, 25.0)
                .alignment(ui::VAttach::Bottom, ui::HAttach::Right)
                .attach(&mut *back.borrow_mut());
            {
                let mut btn = delete_entry.borrow_mut();
                let txt = ui::TextBuilder::new()
                    .text("X")
                    .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                    .attach(&mut *btn);
                btn.add_text(txt);
                let sname = name.clone();
                let saddr = address.clone();
                btn.add_click_func(move |_, game| {
                    let text = format!("Are you sure you wish to delete {} {}?", &sname, &saddr);
                    game.screen_sys.clone().add_screen(Box::new(
                        super::confirm_box::ConfirmBox::new(
                            text,
                            Rc::new(|game| {
                                game.screen_sys.pop_screen();
                            }),
                            Rc::new(move |game| {
                                game.screen_sys.pop_screen();
                                Self::delete_server(index);
                            }),
                        ),
                    ));
                    true
                })
            }

            // Edit entry button
            let edit_entry = ui::ButtonBuilder::new()
                .position(25.0, 0.0)
                .size(25.0, 25.0)
                .alignment(ui::VAttach::Bottom, ui::HAttach::Right)
                .attach(&mut *back.borrow_mut());
            {
                let mut btn = edit_entry.borrow_mut();
                let txt = ui::TextBuilder::new()
                    .text("E")
                    .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                    .attach(&mut *btn);
                btn.add_text(txt);
                let sname = name.clone();
                let saddr = address.clone();
                btn.add_click_func(move |_, game| {
                    game.screen_sys.clone().replace_screen(Box::new(
                        super::edit_server::EditServerEntry::new(Some((
                            index,
                            sname.clone(),
                            saddr.clone(),
                        ))),
                    ));
                    true
                })
            }

            let mut server = Server {
                back,
                offset,
                y: 0.0,
                hovered,
                done_ping: false,
                recv,

                motd,
                ping,
                players,
                version,

                icon,
                icon_texture: None,
            };
            server.update_position();
            elements.servers.push(server);
            offset += 1.0;

            // Don't block the main thread whilst pinging the server
            thread::spawn(move || {
                match protocol::Conn::new(&address, protocol::SUPPORTED_PROTOCOLS[0])
                    .and_then(|conn| conn.do_status())
                {
                    Ok(res) => {
                        let desc = res.0.description;
                        let favicon = if let Some(icon) = res.0.favicon {
                            let data_base64 = &icon["data:image/png;base64,".len()..];
                            let data_base64: String =
                                data_base64.chars().filter(|c| !c.is_whitespace()).collect();
                            let data = STANDARD.decode(data_base64).unwrap();
                            Some(image::load_from_memory(&data).unwrap())
                        } else {
                            None
                        };
                        drop(send.send(PingInfo {
                            motd: desc,
                            ping: res.1,
                            exists: true,
                            online: res.0.players.online,
                            max: res.0.players.max,
                            protocol_version: res.0.version.protocol,
                            protocol_name: res.0.version.name,
                            forge_mods: res.0.forge_mods,
                            favicon,
                        }));
                    }
                    Err(err) => {
                        let e = format!("{}", err);
                        let msg = ComponentType::new(&e, Some(format::Color::Red));
                        let _ = send.send(PingInfo {
                            motd: Component::new(msg),
                            ping: Duration::new(99999, 0),
                            exists: false,
                            online: 0,
                            max: 0,
                            protocol_version: 0,
                            protocol_name: "".to_owned(),
                            forge_mods: vec![],
                            favicon: None,
                        });
                    }
                }
            });
        }
    }

    fn delete_server(index: usize) {
        let mut servers_info = match fs::File::open(paths::get_data_dir().join("servers.json")) {
            Ok(val) => serde_json::from_reader(val).unwrap(),
            Err(_) => {
                let mut info = BTreeMap::default();
                info.insert("servers".to_owned(), Value::Array(vec![]));
                Value::Object(info.into_iter().collect())
            }
        };

        {
            let servers = servers_info
                .as_object_mut()
                .unwrap()
                .get_mut("servers")
                .unwrap()
                .as_array_mut()
                .unwrap();
            servers.remove(index);
        }

        let mut out = fs::File::create(paths::get_data_dir().join("servers.json")).unwrap();
        serde_json::to_writer_pretty(&mut out, &servers_info).unwrap();
    }

    fn init_list(&mut self, renderer: Arc<render::Renderer>, ui_container: &mut ui::Container) {
        let logo = ui::logo::Logo::new(renderer.resources.clone(), ui_container);

        let nr = self.needs_reload.clone();
        let sel = self.selected.clone();

        // Join the currently selected server
        let join = ui::ButtonBuilder::new()
            .position(-122.0, 110.0)
            .size(120.0, 30.0)
            .disabled(true)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut join = join.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Join Server")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *join);
            join.add_text(txt);
            let sel = sel.clone();
            join.add_click_func(move |_, game| {
                let selected = sel.borrow().clone();
                if let Some((_, _, address)) = selected {
                    game.screen_sys
                        .clone()
                        .replace_screen(Box::new(super::connecting::Connecting::new(&address)));
                    let hud_context = Arc::new(RwLock::new(HudContext::new()));
                    let result = game.connect_to(&address, hud_context.clone());
                    game.screen_sys.clone().pop_screen();
                    if let Err(error) = result {
                        game.screen_sys.clone().add_screen(Box::new(ServerList::new(Some(
                            Component::new(ComponentType::new(&error.to_string(), None)),
                        ))));
                    } else {
                        game.screen_sys
                            .clone()
                            .add_screen(Box::new(Hud::new(hud_context)));
                    }
                }
                true
            });
        }

        // Connect directly to a server by address
        let direct = ui::ButtonBuilder::new()
            .position(0.0, 110.0)
            .size(120.0, 30.0)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut direct = direct.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Direct Connection")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *direct);
            direct.add_text(txt);
            direct.add_click_func(|_, game| {
                game.screen_sys.clone().replace_screen(Box::new(
                    super::direct_connect::DirectConnection::new(),
                ));
                true
            });
        }

        // Add a new server to the list
        let add = ui::ButtonBuilder::new()
            .position(122.0, 110.0)
            .size(120.0, 30.0)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut add = add.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Add Server")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *add);
            add.add_text(txt);
            add.add_click_func(move |_, game| {
                game.screen_sys
                    .clone()
                    .replace_screen(Box::new(super::edit_server::EditServerEntry::new(None)));
                true
            })
        }

        // Edit the currently selected server
        let edit = ui::ButtonBuilder::new()
            .position(-184.0, 75.0)
            .size(120.0, 30.0)
            .disabled(true)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut edit = edit.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Edit")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *edit);
            edit.add_text(txt);
            let sel = sel.clone();
            edit.add_click_func(move |_, game| {
                let selected = sel.borrow().clone();
                if let Some((index, name, address)) = selected {
                    game.screen_sys.clone().replace_screen(Box::new(
                        super::edit_server::EditServerEntry::new(Some((index, name, address))),
                    ));
                }
                true
            });
        }

        // Delete the currently selected server
        let delete = ui::ButtonBuilder::new()
            .position(-60.0, 75.0)
            .size(120.0, 30.0)
            .disabled(true)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut delete = delete.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Delete")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *delete);
            delete.add_text(txt);
            let nr = nr.clone();
            let sel = sel.clone();
            delete.add_click_func(move |_, game| {
                let selected = sel.borrow().clone();
                if let Some((index, name, address)) = selected {
                    let text = format!("Are you sure you wish to delete {} {}?", name, address);
                    let sel = sel.clone();
                    let nr = nr.clone();
                    game.screen_sys.clone().add_screen(Box::new(
                        super::confirm_box::ConfirmBox::new(
                            text,
                            Rc::new(|game| {
                                game.screen_sys.pop_screen();
                            }),
                            Rc::new(move |game| {
                                game.screen_sys.pop_screen();
                                Self::delete_server(index);
                                *sel.borrow_mut() = None;
                                *nr.borrow_mut() = true;
                            }),
                        ),
                    ));
                }
                true
            });
        }

        // Refresh the server list
        let refresh = ui::ButtonBuilder::new()
            .position(64.0, 75.0)
            .size(120.0, 30.0)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut refresh = refresh.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Refresh")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *refresh);
            refresh.add_text(txt);
            let nr = nr.clone();
            refresh.add_click_func(move |_, _| {
                *nr.borrow_mut() = true;
                true
            })
        }

        // Back to the main menu
        let back = ui::ButtonBuilder::new()
            .position(188.0, 75.0)
            .size(120.0, 30.0)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Center)
            .draw_index(2)
            .create(ui_container);
        {
            let mut back = back.borrow_mut();
            let txt = ui::TextBuilder::new()
                .text("Back")
                .scale_x(0.7)
                .scale_y(0.7)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *back);
            back.add_text(txt);
            back.add_click_func(|_, game| {
                game.screen_sys
                    .clone()
                    .replace_screen(Box::new(super::main_menu::MainMenu::new()));
                true
            });
        }

        // Options menu
        let options = ui::ButtonBuilder::new()
            .position(5.0, 25.0)
            .size(40.0, 40.0)
            .draw_index(1)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Right)
            .create(ui_container);
        {
            let mut options = options.borrow_mut();
            ui::ImageBuilder::new()
                .texture("rustcraft:gui/cog")
                .position(0.0, 0.0)
                .size(40.0, 40.0)
                .alignment(ui::VAttach::Middle, ui::HAttach::Center)
                .attach(&mut *options);
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

        // Disclaimer
        let disclaimer = ui::TextBuilder::new()
            .text("Not affiliated with Mojang/Minecraft")
            .position(5.0, 5.0)
            .colour((255, 200, 200, 255))
            .draw_index(1)
            .alignment(ui::VAttach::Bottom, ui::HAttach::Right)
            .create(ui_container);

        // If we are kicked from a server display the reason
        let disconnected = if let Some(ref disconnect_reason) = self.disconnect_reason {
            let (width, height) = ui::Formatted::compute_size(
                renderer.clone(),
                disconnect_reason,
                600.0,
                1.0,
                1.0,
                1.0,
            );
            let background = ui::ImageBuilder::new()
                .texture("rustcraft:solid")
                .position(0.0, 3.0)
                .size(
                    width.max(renderer.ui.lock().size_of_string("Disconnected")) + 4.0,
                    height + 4.0 + 16.0,
                )
                .colour((0, 0, 0, 100))
                .alignment(ui::VAttach::Top, ui::HAttach::Center)
                .draw_index(10)
                .create(ui_container);
            ui::TextBuilder::new()
                .text("Disconnected")
                .position(0.0, 2.0)
                .colour((255, 0, 0, 255))
                .alignment(ui::VAttach::Top, ui::HAttach::Center)
                .attach(&mut *background.borrow_mut());
            ui::FormattedBuilder::new()
                .text(disconnect_reason.clone())
                .position(0.0, 18.0)
                .max_width(600.0)
                .alignment(ui::VAttach::Top, ui::HAttach::Center)
                .attach(&mut *background.borrow_mut());
            Some(background)
        } else {
            None
        };

        self.elements = Some(UIElements {
            logo,
            servers: vec![],
            selected: self.selected.clone(),

            _join_btn: join,
            _direct_btn: direct,
            _add_btn: add,
            _edit_btn: edit,
            _delete_btn: delete,
            _refresh_btn: refresh,
            _back_btn: back,
            _options_btn: options,
            _disclaimer: disclaimer,

            _disconnected: disconnected,
        });
    }
}

impl super::Screen for ServerList {
    fn on_active(
        &mut self,
        _screen_sys: &ScreenSystem,
        renderer: Arc<render::Renderer>,
        ui_container: &mut ui::Container,
    ) {
        self.init_list(renderer, ui_container);
        *self.needs_reload.borrow_mut() = true;
    }

    fn on_deactive(
        &mut self,
        _screen_sys: &ScreenSystem,
        renderer: Arc<render::Renderer>,
        _ui_container: &mut ui::Container,
    ) {
        // Clean up
        {
            let elements = self.elements.as_mut().unwrap();
            let mut tex = renderer.get_textures_ref().write();
            for server in &mut elements.servers {
                if let Some(ref icon) = server.icon_texture {
                    tex.remove_dynamic(icon);
                }
            }
        }
        self.elements = None
    }

    fn tick(
        &mut self,
        _screen_sys: &ScreenSystem,
        renderer: Arc<render::Renderer>,
        ui_container: &mut ui::Container,
        delta: f64,
    ) {
        if *self.needs_reload.borrow() {
            self.reload_server_list(renderer.clone(), ui_container);
        }
        let elements = self.elements.as_mut().unwrap();

        elements.logo.tick(renderer.clone());

        for s in &mut elements.servers {
            // Animate the entries
            {
                let mut back = s.back.borrow_mut();
                let dy = s.y - back.y;
                if dy * dy > 1.0 {
                    let y = back.y;
                    back.y = y + delta * dy * 0.1;
                } else {
                    back.y = s.y;
                }
            }
            #[allow(clippy::if_same_then_else)]
            if s.y < elements._join_btn.borrow().y {
                // TODO: Make button invisible!
            } else {
                // TODO: Make button visible.
            }

            // Keep checking to see if the server has finished being
            // pinged
            if !s.done_ping {
                match s.recv.try_recv() {
                    Ok(res) => {
                        s.done_ping = true;
                        s.motd.borrow_mut().set_text(res.motd);
                        // Selects the icon for the given ping range
                        // TODO: switch to as_millis() experimental duration_as_u128 #50202 once available?
                        let ping_ms = (res.ping.subsec_nanos() as f64) / 1000000.0
                            + (res.ping.as_secs() as f64) * 1000.0;
                        let y = match ping_ms.round() as u64 {
                            _x @ 0..=75 => 16.0,
                            _x @ 76..=150 => 24.0,
                            _x @ 151..=225 => 32.0,
                            _x @ 226..=350 => 40.0,
                            _x @ 351..=999 => 48.0,
                            _ => 56.0,
                        };
                        s.ping.borrow_mut().texture_coords.1 = y;
                        if res.exists {
                            {
                                let mut players = s.players.borrow_mut();
                                let txt = if protocol::SUPPORTED_PROTOCOLS
                                    .contains(&res.protocol_version)
                                {
                                    players.colour.1 = 255;
                                    players.colour.2 = 255;
                                    format!("{}/{}", res.online, res.max)
                                } else {
                                    players.colour.1 = 85;
                                    players.colour.2 = 85;
                                    format!("Out of date {}/{}", res.online, res.max)
                                };
                                players.text = txt;
                            }
                            let sm =
                                format!("{} mods + {}", res.forge_mods.len(), res.protocol_name);
                            let st = if !res.forge_mods.is_empty() {
                                &sm
                            } else {
                                &res.protocol_name
                            };
                            let msg_component =
                                Component::new(ComponentType::new(st, Some(format::Color::Yellow)));
                            s.version.borrow_mut().set_text(msg_component);
                        }
                        if let Some(favicon) = res.favicon {
                            let name: String = std::iter::repeat(())
                                .map(|()| {
                                    rand::thread_rng().sample(rand::distributions::Alphanumeric)
                                        as char
                                })
                                .take(30)
                                .collect();
                            let tex = renderer.get_textures_ref();
                            s.icon_texture = Some(name.clone());
                            let icon_tex = tex.write().put_dynamic(&name, favicon);
                            s.icon.borrow_mut().texture = icon_tex.name;
                        }
                    }
                    Err(TryRecvError::Disconnected) => {
                        s.done_ping = true;
                        s.motd
                            .borrow_mut()
                            .set_text(Component::new(ComponentType::new(
                                "Channel dropped",
                                Some(format::Color::Red),
                            )));
                    }
                    _ => {}
                }
            }
        }

        // Sync the selection highlight and the disabled state of the
        // selection-dependent buttons
        let selected_index = elements.selected.borrow().as_ref().map(|s| s.0);
        for (index, s) in elements.servers.iter().enumerate() {
            let mut back = s.back.borrow_mut();
            if selected_index == Some(index) {
                back.colour = (0, 110, 0, 230);
            } else if *s.hovered.borrow() {
                back.colour = (0, 0, 0, 200);
            } else {
                back.colour = (0, 0, 0, 100);
            }
        }
        let disabled = selected_index.is_none();
        for btn in [&elements._join_btn, &elements._edit_btn, &elements._delete_btn] {
            btn.borrow_mut().disabled = disabled;
        }
    }

    fn on_scroll(&mut self, _: f64, y: f64) {
        let elements = self.elements.as_mut().unwrap();
        if elements.servers.is_empty() {
            return;
        }
        let mut diff = y / 1.0;
        {
            let last = elements.servers.last().unwrap();
            if last.offset + diff <= 2.0 {
                diff = 2.0 - last.offset;
            }
            let first = elements.servers.first().unwrap();
            if first.offset + diff >= 0.0 {
                diff = -first.offset;
            }
        }

        for s in &mut elements.servers {
            s.offset += diff;
            s.update_position();
        }
    }

    fn on_resize(
        &mut self,
        screen_sys: &ScreenSystem,
        renderer: Arc<Renderer>,
        ui_container: &mut Container,
    ) {
        // TODO: Don't ping the servers on resize!
        self.on_deactive(screen_sys, renderer.clone(), ui_container);
        self.on_active(screen_sys, renderer, ui_container);
    }

    fn clone_screen(&self) -> Box<dyn Screen> {
        Box::new(self.clone())
    }
}
