use super::{
    Bounds, Digging, GameInfo, Gravity, Light, MouseButtons, Position, Rotation, TargetPosition,
    TargetRotation, Velocity,
};
use crate::ecs::{Manager, SystemExecStage};
use crate::entity::slime::{added_slime, update_slime};
use crate::entity::zombie::{added_zombie, update_zombie};
use crate::entity::{resolve_textures, EntityType};
use crate::format;
use crate::render;
use crate::render::model::{self, FormatState};
use crate::render::Renderer;
use crate::server::{RendererResource, ScreenSystemResource, WorldResource};
use crate::settings::Actionkey;
use crate::shared::Position as BPosition;
use crate::types::hash::FNVHash;
use crate::types::GameMode;
use crate::world;
use arc_swap::ArcSwapOption;
use bevy_ecs::prelude::*;
use cgmath::{Decomposed, Matrix4, Point3, Quaternion, Rad, Rotation3, SquareMatrix, Vector3};
use collision::{Aabb, Aabb3};
use instant::Instant;
use rustcraft_protocol::format::Component;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn add_systems(sched: &mut Schedule, render_sched: &mut Schedule) {
    // TODO: Check sync/async usage!
    sched.add_systems(
        handle_movement
            .in_set(SystemExecStage::Render)
            .after(SystemExecStage::Normal),
    );
    // let sys = ParticleRenderer::new(m);
    // m.add_render_system(sys);
    render_sched
        .add_systems(
            update_render_players
                .in_set(SystemExecStage::Render)
                .after(SystemExecStage::Normal),
        )
        .add_systems(
            player_added
                .in_set(SystemExecStage::Render)
                .after(SystemExecStage::Normal),
        )
        .add_systems(
            update_slime
                .in_set(SystemExecStage::Render)
                .after(SystemExecStage::Normal),
        )
        .add_systems(
            added_slime
                .in_set(SystemExecStage::Render)
                .after(SystemExecStage::Normal),
        )
        .add_systems(
            update_zombie
                .in_set(SystemExecStage::Render)
                .after(SystemExecStage::Normal),
        )
        .add_systems(
            added_zombie
                .in_set(SystemExecStage::Render)
                .after(SystemExecStage::Normal),
        );
}

pub fn create_local(m: &mut Manager) -> Entity {
    let mut entity = m.world.spawn_empty();
    let mut tpos = TargetPosition::new(0.0, 0.0, 0.0);
    tpos.lerp_amount = 0.9;
    entity
        .insert(Position::new(0.0, 0.0, 0.0))
        .insert(tpos)
        .insert(Rotation::new(0.0, 0.0))
        .insert(Velocity::new(0.0, 0.0, 0.0))
        .insert(GameMode::Survival)
        .insert(Gravity::new())
        .insert(PlayerMovement::new())
        .insert(Bounds::new(Aabb3::new(
            Point3::new(-0.3, 0.0, -0.3),
            Point3::new(0.3, 1.8, 0.3),
        )))
        .insert(PlayerModel::new(
            Component::from_str(""),
            false,
            false,
            true,
        ))
        .insert(Light::new())
        .insert(Digging::new())
        .insert(MouseButtons::new())
        .insert(MovementDelta::default())
        .insert(EntityType::Player);
    entity.id()
}

pub fn create_remote(m: &mut Manager, name: Component) -> Entity {
    let mut entity = m.world.spawn_empty();
    entity
        .insert(Position::new(0.0, 0.0, 0.0))
        .insert(TargetPosition::new(0.0, 0.0, 0.0))
        .insert(Rotation::new(0.0, 0.0))
        .insert(TargetRotation::new(0.0, 0.0))
        .insert(Velocity::new(0.0, 0.0, 0.0))
        .insert(Bounds::new(Aabb3::new(
            Point3::new(-0.3, 0.0, -0.3),
            Point3::new(0.3, 1.8, 0.3),
        )))
        .insert(PlayerModel::new(name, true, true, false))
        .insert(Light::new())
        .insert(EntityType::Player);
    entity.id()
}

#[derive(Component)]
pub struct PlayerModel {
    model: Option<model::ModelHandle>,
    skin_url: ArcSwapOption<String>,
    dirty: AtomicBool,
    display_name: Component,

    has_head: bool,
    has_name_tag: bool,
    first_person: bool,

    dir: i32,
    time: f64,
    still_time: f64,
    idle_time: f64,
    arm_time: f64,
}

impl PlayerModel {
    pub fn new(name: Component, has_head: bool, has_name_tag: bool, first_person: bool) -> Self {
        Self {
            model: None,
            skin_url: ArcSwapOption::new(None),
            dirty: AtomicBool::new(false),
            display_name: name,

            has_head,
            has_name_tag,
            first_person,

            dir: 0,
            time: 0.0,
            still_time: 0.0,
            idle_time: 0.0,
            arm_time: 0.0,
        }
    }

    pub fn set_skin(&mut self, skin: Option<String>) {
        if self.skin_url.load().as_ref().map(|skin| skin.as_ref()) != skin.as_ref() {
            self.skin_url.store(skin.map(Arc::new));
            self.dirty.store(true, Ordering::Release);
        }
    }
}

fn update_render_players(
    renderer: Res<RendererResource>,
    game_info: Res<GameInfo>,
    mut query: Query<(&mut PlayerModel, &Position, &Rotation, &Light, Option<&MouseButtons>)>,
) {
    let renderer = &renderer.0;
    let delta = game_info.delta;
    for (mut player_model, position, rotation, light, mouse_buttons) in query.iter_mut() {
        use std::f32::consts::PI;
        use std::f64::consts::PI as PI64;

        if player_model.dirty.load(Ordering::Acquire) {
            add_player(renderer.clone(), &mut player_model);
        }

        if let Some(pmodel) = &player_model.model {
            let renderer = renderer.clone();
            let cam_x = renderer.camera.lock().pos.x;
            let cam_z = renderer.camera.lock().pos.z;
            let mut models = renderer.models.lock();
            let mdl = models.get_model(pmodel).unwrap();

            mdl.block_light = light.block_light;
            mdl.sky_light = light.sky_light;
            mdl.no_cull = player_model.first_person;

            let offset = if player_model.first_person {
                let ox = (rotation.yaw - PI64 / 2.0).cos() * 0.25;
                let oz = -(rotation.yaw - PI64 / 2.0).sin() * 0.25;
                Vector3::new(
                    position.position.x as f32 - ox as f32,
                    -position.position.y as f32,
                    position.position.z as f32 - oz as f32,
                )
            } else {
                Vector3::new(
                    position.position.x as f32,
                    -position.position.y as f32,
                    position.position.z as f32,
                )
            };
            let offset_matrix = Matrix4::from(Decomposed {
                scale: 1.0,
                rot: Quaternion::from_angle_y(Rad(PI + rotation.yaw as f32)),
                disp: offset,
            });

            let first_person_offset = if player_model.first_person {
                let cam = renderer.camera.lock();
                let cam_pos = Vector3::new(cam.pos.x as f32, -(cam.pos.y as f32), cam.pos.z as f32);
                let forward = Vector3::new(
                    (rotation.yaw as f32 - PI / 2.0).cos() * 0.5,
                    0.0,
                    -(rotation.yaw as f32 - PI / 2.0).sin() * 0.5,
                );
                Some(Matrix4::from(Decomposed {
                    scale: 1.0,
                    rot: Quaternion::from_angle_y(Rad(PI + rotation.yaw as f32)),
                    disp: cam_pos + forward,
                }))
            } else {
                None
            };

            // TODO This sucks
            if player_model.has_name_tag {
                let ang = (position.position.x - cam_x).atan2(position.position.z - cam_z) as f32;
                mdl.matrix[PlayerModelPart::NameTag as usize] = Matrix4::from(Decomposed {
                    scale: 1.0,
                    rot: Quaternion::from_angle_y(Rad(ang)),
                    disp: offset + Vector3::new(0.0, (-24.0 / 16.0) - 0.6, 0.0),
                });
            }

            mdl.matrix[PlayerModelPart::Head as usize] = offset_matrix
                * Matrix4::from(Decomposed {
                    scale: 1.0,
                    rot: Quaternion::from_angle_x(Rad(-rotation.pitch as f32)),
                    disp: Vector3::new(0.0, -12.0 / 16.0 - 12.0 / 16.0, 0.0),
                });
            mdl.matrix[PlayerModelPart::Body as usize] = offset_matrix
                * Matrix4::from(Decomposed {
                    scale: 1.0,
                    rot: Quaternion::from_angle_x(Rad(0.0)),
                    disp: Vector3::new(0.0, -12.0 / 16.0 - 6.0 / 16.0, 0.0),
                });

            let mut time = player_model.time;
            let mut dir = player_model.dir;
            if dir == 0 {
                dir = 1;
                time = 15.0;
            }
            let ang = ((time / 15.0) - 1.0) * (PI64 / 4.0);

            mdl.matrix[PlayerModelPart::LegRight as usize] = offset_matrix
                * Matrix4::from(Decomposed {
                    scale: 1.0,
                    rot: Quaternion::from_angle_x(Rad(ang as f32)),
                    disp: Vector3::new(2.0 / 16.0, -12.0 / 16.0, 0.0),
                });
            mdl.matrix[PlayerModelPart::LegLeft as usize] = offset_matrix
                * Matrix4::from(Decomposed {
                    scale: 1.0,
                    rot: Quaternion::from_angle_x(Rad(-ang as f32)),
                    disp: Vector3::new(-2.0 / 16.0, -12.0 / 16.0, 0.0),
                });

            let mut i_time = player_model.idle_time;
            i_time += delta * 0.02;
            if i_time > PI64 * 2.0 {
                i_time -= PI64 * 2.0;
            }
            player_model.idle_time = i_time;

            if let Some(mouse_buttons) = mouse_buttons {
                if mouse_buttons.left {
                    if player_model.arm_time <= 0.0 {
                        player_model.arm_time = 15.0;
                    }
                }
            }

            if player_model.arm_time <= 0.0 {
                player_model.arm_time = 0.0;
            } else {
                player_model.arm_time -= delta;
            }

            let arm_base = first_person_offset.as_ref().unwrap_or(&offset_matrix);

            if player_model.first_person {
                let v_inv = renderer.camera_matrix.lock().invert().unwrap_or(Matrix4::identity());
                let punch_forward = ((7.5 - (player_model.arm_time - 7.5).abs()) / 7.5) as f32;
                let hook_sweep = punch_forward;
                let hand_pos = Vector3::new(0.15 - hook_sweep * 0.075, -0.25 - punch_forward * 0.05, -0.01 - punch_forward * 0.15);

                mdl.matrix[PlayerModelPart::ArmRight as usize] = v_inv
                    * Matrix4::from_translation(hand_pos)
                    * Matrix4::from_scale(0.30)
                    * Matrix4::from(Quaternion::from_angle_y(Rad(-0.25)))
                    * Matrix4::from(Quaternion::from_angle_x(Rad(-1.05)))
                    * Matrix4::from(Quaternion::from_angle_z(Rad(
                        (i_time.cos() * 0.06 - 0.06 + 0.2 + hook_sweep as f64 * 0.5236) as f32
                    )))
                    * Matrix4::from(Quaternion::from_angle_x(Rad(
                        (i_time.sin() * 0.06) as f32
                    )));

                mdl.matrix[PlayerModelPart::ArmLeft as usize] = Matrix4::identity();
            } else {
                mdl.matrix[PlayerModelPart::ArmRight as usize] = arm_base
                    * Matrix4::from_translation(Vector3::new(
                        6.0 / 16.0,
                        -12.0 / 16.0 - 12.0 / 16.0,
                        0.0,
                    ))
                    * Matrix4::from(Quaternion::from_angle_x(Rad(-(ang * 0.75) as f32)))
                    * Matrix4::from(Quaternion::from_angle_z(Rad(
                        (i_time.cos() * 0.06 - 0.06) as f32
                    )))
                    * Matrix4::from(Quaternion::from_angle_x(Rad((i_time.sin() * 0.06
                        - ((7.5 - (player_model.arm_time - 7.5).abs()) / 7.5))
                        as f32)));

                mdl.matrix[PlayerModelPart::ArmLeft as usize] = arm_base
                    * Matrix4::from_translation(Vector3::new(
                        -6.0 / 16.0,
                        -12.0 / 16.0 - 12.0 / 16.0,
                        0.0,
                    ))
                    * Matrix4::from(Quaternion::from_angle_x(Rad((ang * 0.75) as f32)))
                    * Matrix4::from(Quaternion::from_angle_z(Rad(
                        -(i_time.cos() * 0.06 - 0.06) as f32
                    )))
                    * Matrix4::from(Quaternion::from_angle_x(Rad(-(i_time.sin() * 0.06) as f32)));
            }

            let mut update = true;
            if position.moved {
                player_model.still_time = 0.0;
            } else if player_model.still_time > 2.0 {
                if (time - 15.0).abs() <= 1.5 * delta {
                    time = 15.0;
                    update = false;
                }
                dir = (15.0 - time).signum() as i32;
            } else {
                player_model.still_time += delta;
            }

            if update {
                time += delta * 1.5 * (dir as f64);
                if time > 30.0 {
                    time = 30.0;
                    dir = -1;
                } else if time < 0.0 {
                    time = 0.0;
                    dir = 1;
                }
            }
            player_model.time = time;
            player_model.dir = dir;
        }
    }
}

pub fn player_added(
    renderer: Res<RendererResource>,
    mut query: Query<&mut PlayerModel, Added<PlayerModel>>,
) {
    let renderer = &renderer.0;
    for mut player_model in query.iter_mut() {
        add_player(renderer.clone(), &mut player_model);
    }
}

// TODO: Setup culling
fn add_player(renderer: Arc<Renderer>, player_model: &mut PlayerModel) {
    player_model.dirty.store(false, Ordering::Release);

    let skin = if let Some(url) = player_model.skin_url.load().as_ref() {
        renderer.get_skin(renderer.get_textures_ref(), url)
    } else {
        render::Renderer::get_texture(renderer.get_textures_ref(), "entity/steve")
    };

    // TODO: Replace this shit entirely!
    macro_rules! srel {
        ($x:expr, $y:expr, $w:expr, $h:expr) => {
            Some(skin.relative(($x) / 64.0, ($y) / 64.0, ($w) / 64.0, ($h) / 64.0))
        };
    }

    let mut head_verts = vec![];
    if player_model.has_head {
        model::append_box(
            &mut head_verts,
            -4.0 / 16.0,
            0.0,
            -4.0 / 16.0,
            8.0 / 16.0,
            8.0 / 16.0,
            8.0 / 16.0,
            resolve_textures(&skin, 8.0, 8.0, 8.0, 0.0, 0.0),
        );
        model::append_box(
            &mut head_verts,
            -4.2 / 16.0,
            -0.2 / 16.0,
            -4.2 / 16.0,
            8.4 / 16.0,
            8.4 / 16.0,
            8.4 / 16.0,
            resolve_textures(&skin, 8.0, 8.0, 8.0, 32.0, 0.0),
        );
    }

    // TODO: Cape
    let mut body_verts = vec![];
    if !player_model.first_person {
        model::append_box(
            &mut body_verts,
            -4.0 / 16.0,
            -6.0 / 16.0,
            -2.0 / 16.0,
            8.0 / 16.0,
            12.0 / 16.0,
            4.0 / 16.0,
            resolve_textures(&skin, 8.0, 12.0, 4.0, 16.0, 16.0),
        );
        model::append_box(
            &mut body_verts,
            -4.2 / 16.0,
            -6.2 / 16.0,
            -2.2 / 16.0,
            8.4 / 16.0,
            12.4 / 16.0,
            4.4 / 16.0,
            resolve_textures(&skin, 8.0, 12.0, 4.0, 16.0, 16.0),
        );
    }

    let mut part_verts = vec![vec![]; 4];

    for (i, offsets) in [
        [16.0, 48.0, 0.0, 48.0],  // Left leg
        [0.0, 16.0, 0.0, 32.0],   // Right Leg
        [32.0, 48.0, 48.0, 48.0], // Left arm
        [40.0, 16.0, 40.0, 32.0], // Right arm
    ]
    .iter()
    .enumerate()
    {
        if player_model.first_person && i < 2 {
            continue;
        }
        // TODO: Fix alex (slim) skins
        let alex = false;
        let width = if alex {
            // arms of alex (slim) skins have 3/4 of the width of normal skins!
            3.0
        } else {
            4.0
        };
        let (ox, oy) = (offsets[0], offsets[1]);
        model::append_box(
            &mut part_verts[i],
            -2.0 / 16.0,
            -12.0 / 16.0,
            -2.0 / 16.0,
            4.0 / 16.0,
            12.0 / 16.0,
            4.0 / 16.0,
            [
                srel!(ox + 8.0, oy + 0.0, 4.0, 4.0),     // Down
                srel!(ox + 4.0, oy + 0.0, 4.0, 4.0),     // Up
                srel!(ox + 4.0, oy + 4.0, width, 12.0),  // North
                srel!(ox + 12.0, oy + 4.0, width, 12.0), // South
                srel!(ox + 8.0, oy + 4.0, width, 12.0),  // West
                srel!(ox + 0.0, oy + 4.0, width, 12.0),  // East
            ],
        );
        let (ox, oy) = (offsets[2], offsets[3]);
        model::append_box(
            &mut part_verts[i],
            -2.2 / 16.0,
            -12.2 / 16.0,
            -2.2 / 16.0,
            4.4 / 16.0,
            12.4 / 16.0,
            4.4 / 16.0,
            [
                srel!(ox + 8.0, oy + 0.0, 4.0, 4.0),   // Down
                srel!(ox + 4.0, oy + 0.0, 4.0, 4.0),   // Up
                srel!(ox + 4.0, oy + 4.0, 4.0, 12.0),  // North
                srel!(ox + 12.0, oy + 4.0, 4.0, 12.0), // South
                srel!(ox + 8.0, oy + 4.0, 4.0, 12.0),  // West
                srel!(ox + 0.0, oy + 4.0, 4.0, 12.0),  // East
            ],
        );
    }

    let mut name_verts = vec![];
    if player_model.has_name_tag {
        let mut state = FormatState {
            width: 0.0,
            offset: 0.0,
            text: Vec::new(),
            renderer: renderer.clone(),
            y_scale: 0.16,
            x_scale: 0.01,
        };
        state.build(&player_model.display_name, Some(format::Color::Black));
        // TODO: Remove black shadow and add dark, transparent box around name
        let width = state.width;
        // Center align text
        for vert in &mut state.text {
            vert.x += width * 0.5;
            vert.r = 64;
            vert.g = 64;
            vert.b = 64;
        }
        name_verts.extend_from_slice(&state.text);
        for vert in &mut state.text {
            vert.x -= 0.01;
            vert.y -= 0.01;
            vert.z -= 0.05;
            vert.r = 255;
            vert.g = 255;
            vert.b = 255;
        }
        name_verts.extend_from_slice(&state.text);
    }
    let mut model = renderer.clone().models.lock().create_model(
        model::FIRST_PERSON,
        vec![
            head_verts,
            body_verts,
            part_verts[0].clone(),
            part_verts[1].clone(),
            part_verts[2].clone(),
            part_verts[3].clone(),
            name_verts,
        ],
        renderer,
    );
    let skin_url = player_model.skin_url.load();
    model.2 = player_model.model.as_ref().map_or(
        Some(Arc::new(move |renderer: Arc<Renderer>| {
            let skin_url = skin_url.clone();
            if let Some(url) = skin_url.as_ref() {
                renderer.get_textures_ref().read().release_skin(url); // TODO: Move this into the custom drop handling fn!
            };
        })),
        |x| x.2.clone(),
    );

    player_model.model.replace(model);
}

enum PlayerModelPart {
    Head = 0,
    Body = 1,
    LegLeft = 2,
    LegRight = 3,
    ArmLeft = 4,
    ArmRight = 5,
    NameTag = 6,
    // Cape = 7, // TODO
}

#[derive(Component, Default)]
pub struct PlayerMovement {
    pub flying: bool,
    pub want_to_fly: bool,
    pub when_last_jump_pressed: Option<Instant>,
    pub when_last_jump_released: Option<Instant>,
    pub did_touch_ground: bool,
    pub pressed_keys: HashMap<Actionkey, bool, BuildHasherDefault<FNVHash>>,
}

impl PlayerMovement {
    pub fn new() -> PlayerMovement {
        Default::default()
    }

    fn movement_input(&self) -> (f64, f64) {
        let mut forward = 0.0;
        let mut strafe = 0.0;
        if self.is_key_pressed(Actionkey::Forward) {
            forward += 1.0;
        }
        if self.is_key_pressed(Actionkey::Backward) {
            forward -= 1.0;
        }
        if self.is_key_pressed(Actionkey::Left) {
            strafe += 1.0;
        }
        if self.is_key_pressed(Actionkey::Right) {
            strafe -= 1.0;
        }
        if self.is_key_pressed(Actionkey::Sneak) {
            strafe *= 0.3;
            forward *= 0.3;
        }
        (strafe, forward)
    }

    fn is_key_pressed(&self, key: Actionkey) -> bool {
        self.pressed_keys.get(&key).map_or(false, |v| *v)
    }
}

// FIXME: look at this to review our impl: https://www.mcpk.wiki/wiki/Movement_Formulas
#[allow(clippy::type_complexity)]
#[allow(unused_mut)] // we ignore this warning, as this case seems to be a clippy bug
pub fn handle_movement(
    world: Res<WorldResource>,
    screen_sys: Res<ScreenSystemResource>,
    mut commands: Commands,
    mut query: Query<(
        Entity,
        &mut PlayerMovement,
        &mut TargetPosition,
        &mut Velocity,
        &Bounds,
        &Rotation,
        &GameMode,
        Option<&mut Gravity>,
    )>,
) {
    let world = &world.0;
    let screen_sys = &screen_sys.0;
    for (
        entity,
        mut movement,
        mut position,
        mut velocity,
        bounds,
        rotation,
        gamemode,
        mut gravity,
    ) in query.iter_mut()
    {
        if movement.flying && gravity.is_some() {
            commands.entity(entity).remove::<Gravity>();
        } else if !movement.flying && gravity.is_none() {
            commands.entity(entity).insert(Gravity::new());
        }
        movement.flying |= gamemode.always_fly();
        if !screen_sys.is_current_ingame()
            && (movement.pressed_keys.len() > 1
                || (!movement.pressed_keys.is_empty()
                    && !movement.is_key_pressed(Actionkey::OpenInv)))
        {
            movement.pressed_keys.insert(Actionkey::Backward, false);
            movement.pressed_keys.insert(Actionkey::Forward, false);
            movement.pressed_keys.insert(Actionkey::Right, false);
            movement.pressed_keys.insert(Actionkey::Left, false);
            movement.pressed_keys.insert(Actionkey::Jump, false);
            movement.pressed_keys.insert(Actionkey::Sneak, false);
            movement.pressed_keys.insert(Actionkey::Sprint, false);
        }

        // Detect double-tapping jump to toggle creative flight
        if movement.is_key_pressed(Actionkey::Jump) {
            if movement.when_last_jump_pressed.is_none() {
                movement.when_last_jump_pressed = Some(Instant::now());
                if movement.when_last_jump_released.is_some() {
                    let dt = movement.when_last_jump_pressed.unwrap()
                        - movement.when_last_jump_released.unwrap();
                    if dt.as_secs() == 0 && dt.subsec_millis() <= crate::settings::DOUBLE_JUMP_MS {
                        //info!("double jump! dt={:?} toggle want_to_fly = {}", dt, movement.want_to_fly);

                        if gamemode.can_fly() && !gamemode.always_fly() {
                            movement.flying = !movement.flying;
                            movement.want_to_fly = movement.flying;
                        }
                    }
                }
            }
        } else if movement.when_last_jump_pressed.is_some() {
            movement.when_last_jump_released = Some(Instant::now());
            movement.when_last_jump_pressed = None;
        }

        let player_bounds = bounds.bounds;

        let mut last_position = position.position;

        if world.is_chunk_loaded(
            (position.position.x as i32) >> 4,
            (position.position.z as i32) >> 4,
        ) {
            let (strafe, forward) = movement.movement_input();
            let yaw = rotation.yaw - std::f64::consts::PI / 2.0;
            let on_ground = gravity.as_ref().is_some_and(|v| v.on_ground);
            let is_sprinting = movement.is_key_pressed(Actionkey::Sprint)
                && movement.is_key_pressed(Actionkey::Forward)
                && !movement.is_key_pressed(Actionkey::Backward)
                && !movement.is_key_pressed(Actionkey::Sneak);

            if movement.flying {
                let fly_speed = 0.21585 * 2.5 * if is_sprinting { 0.2806 / 0.21585 } else { 1.0 };
                let mut direction = Vector3::new(0.0, 0.0, 0.0);
                move_relative(&mut direction, strafe, forward, 1.0, yaw);
                velocity.velocity.x = direction.x * fly_speed;
                velocity.velocity.z = direction.z * fly_speed;
                velocity.velocity.y = if movement.is_key_pressed(Actionkey::Jump) {
                    fly_speed
                } else if movement.is_key_pressed(Actionkey::Sneak) {
                    -fly_speed
                } else {
                    0.0
                };
            } else {
                if on_ground && movement.is_key_pressed(Actionkey::Jump) {
                    velocity.velocity.y = 0.42;
                    if is_sprinting {
                        let (bx, bz) = sprint_jump_boost(rotation.yaw);
                        velocity.velocity.x += bx;
                        velocity.velocity.z += bz;
                    }
                }

                let movement_factor = if on_ground {
                    let base_speed = if is_sprinting { 0.13 } else { 0.1 };
                    base_speed * (0.16277136 / (0.546 * 0.546 * 0.546))
                } else {
                    0.02
                };
                move_relative(
                    &mut velocity.velocity,
                    strafe,
                    forward,
                    movement_factor,
                    yaw,
                );
            }

            position.position += velocity.velocity;

            if !gamemode.noclip() {
                let mut target = position.position;
                position.position.y = last_position.y;
                position.position.z = last_position.z;

                // We handle each axis separately to allow for a sliding
                // effect when pushing up against walls.

                let (bounds, xhit) =
                    check_collisions(world, &mut position, &last_position, player_bounds);
                position.position.x = bounds.min.x + 0.3;
                last_position.x = position.position.x;
                if xhit {
                    velocity.velocity.x = 0.0;
                }

                position.position.z = target.z;
                let (bounds, zhit) =
                    check_collisions(world, &mut position, &last_position, player_bounds);
                position.position.z = bounds.min.z + 0.3;
                last_position.z = position.position.z;
                if zhit {
                    velocity.velocity.z = 0.0;
                }

                // Half block jumps
                // Minecraft lets you 'jump' up 0.5 blocks
                // for slabs and stairs (or smaller blocks).
                // Currently we implement this as a teleport to the
                // top of the block if we could move there
                // but this isn't smooth.
                if (xhit || zhit) && gravity.as_ref().map_or(false, |v| v.on_ground) {
                    let mut ox = position.position.x;
                    let mut oz = position.position.z;
                    position.position.x = target.x;
                    position.position.z = target.z;
                    for offset in 1..9 {
                        let mini = player_bounds.add_v(cgmath::Vector3::new(
                            0.0,
                            offset as f64 / 16.0,
                            0.0,
                        ));
                        let (_, hit) = check_collisions(world, &mut position, &last_position, mini);
                        if !hit {
                            target.y += offset as f64 / 16.0;
                            ox = target.x;
                            oz = target.z;
                            break;
                        }
                    }
                    position.position.x = ox;
                    position.position.z = oz;
                }

                position.position.y = target.y;
                let (bounds, yhit) =
                    check_collisions(world, &mut position, &last_position, player_bounds);
                position.position.y = bounds.min.y;
                last_position.y = position.position.y;
                if yhit {
                    velocity.velocity.y = 0.0;
                }

                if let Some(mut gravity) = gravity {
                    let ground =
                        Aabb3::new(Point3::new(-0.3, -0.005, -0.3), Point3::new(0.3, 0.0, 0.3));
                    let prev = gravity.on_ground;
                    let (_, hit) = check_collisions(world, &mut position, &last_position, ground);
                    gravity.on_ground = hit;
                    if !prev && gravity.on_ground {
                        movement.did_touch_ground = true;
                    }
                }
            }

            if !movement.flying {
                velocity.velocity.y -= 0.08;
                if velocity.velocity.y < -3.92 {
                    velocity.velocity.y = -3.92;
                }
                velocity.velocity.y *= 0.98;
                let drag = if on_ground { 0.546 } else { 0.91 };
                velocity.velocity.x *= drag;
                velocity.velocity.z *= drag;
            }

            if velocity.velocity.x.abs() < 0.003 {
                velocity.velocity.x = 0.0;
            }
            if velocity.velocity.y.abs() < 0.003 {
                velocity.velocity.y = 0.0;
            }
            if velocity.velocity.z.abs() < 0.003 {
                velocity.velocity.z = 0.0;
            }
        }
    }
}

fn move_relative(
    velocity: &mut Vector3<f64>,
    strafe: f64,
    forward: f64,
    movement_factor: f64,
    yaw: f64,
) {
    let mut distance = strafe * strafe + forward * forward;
    if distance >= 1.0e-4 {
        distance = distance.sqrt();
        if distance < 1.0 {
            distance = 1.0;
        }
        let scale = movement_factor / distance;
        let strafe = strafe * scale;
        let forward = forward * scale;
        velocity.x += forward * yaw.cos() - strafe * yaw.sin();
        velocity.z += -forward * yaw.sin() - strafe * yaw.cos();
    }
}

// The horizontal burst of speed applied on a sprint-jump.
fn sprint_jump_boost(yaw: f64) -> (f64, f64) {
    (yaw.sin() * 0.2, yaw.cos() * 0.2)
}

fn check_collisions(
    world: &world::World,
    position: &mut TargetPosition,
    last_position: &Vector3<f64>,
    bounds: Aabb3<f64>,
) -> (Aabb3<f64>, bool) {
    let mut bounds = bounds.add_v(position.position);

    let dir = position.position - last_position;

    let min_x = (bounds.min.x - 1.0) as i32;
    let min_y = (bounds.min.y - 1.0) as i32;
    let min_z = (bounds.min.z - 1.0) as i32;
    let max_x = (bounds.max.x + 1.0) as i32;
    let max_y = (bounds.max.y + 1.0) as i32;
    let max_z = (bounds.max.z + 1.0) as i32;

    let mut hit = false;
    for y in min_y..max_y {
        for z in min_z..max_z {
            for x in min_x..max_x {
                let block = world.get_block(BPosition::new(x, y, z));
                if block.get_material().collidable {
                    for bb in block.get_collision_boxes() {
                        let bb = bb.add_v(cgmath::Vector3::new(x as f64, y as f64, z as f64));
                        if bb.collides(&bounds) {
                            bounds = bounds.move_out_of(bb, dir);
                            hit = true;
                        }
                    }
                }
            }
        }
    }

    (bounds, hit)
}

trait Collidable<T> {
    fn collides(&self, t: &T) -> bool;
    fn move_out_of(self, other: Self, dir: cgmath::Vector3<f64>) -> Self;
}

impl Collidable<Aabb3<f64>> for Aabb3<f64> {
    fn collides(&self, t: &Aabb3<f64>) -> bool {
        !(t.min.x >= self.max.x
            || t.max.x <= self.min.x
            || t.min.y >= self.max.y
            || t.max.y <= self.min.y
            || t.min.z >= self.max.z
            || t.max.z <= self.min.z)
    }

    fn move_out_of(mut self, other: Self, dir: cgmath::Vector3<f64>) -> Self {
        if dir.x != 0.0 {
            if dir.x > 0.0 {
                let ox = self.max.x;
                self.max.x = other.min.x - 0.0001;
                self.min.x += self.max.x - ox;
            } else {
                let ox = self.min.x;
                self.min.x = other.max.x + 0.0001;
                self.max.x += self.min.x - ox;
            }
        }
        if dir.y != 0.0 {
            if dir.y > 0.0 {
                let oy = self.max.y;
                self.max.y = other.min.y - 0.0001;
                self.min.y += self.max.y - oy;
            } else {
                let oy = self.min.y;
                self.min.y = other.max.y + 0.0001;
                self.max.y += self.min.y - oy;
            }
        }
        if dir.z != 0.0 {
            if dir.z > 0.0 {
                let oz = self.max.z;
                self.max.z = other.min.z - 0.0001;
                self.min.z += self.max.z - oz;
            } else {
                let oz = self.min.z;
                self.min.z = other.max.z + 0.0001;
                self.max.z += self.min.z - oz;
            }
        }
        self
    }
}

#[derive(Component)]
pub struct MovementDelta {
    pub prev_pos: Vector3<f64>,
    pub prev_rot: Rotation,
}

impl Default for MovementDelta {
    fn default() -> Self {
        Self {
            prev_pos: Vector3::new(f64::MAX, f64::MAX, f64::MAX),
            prev_rot: Rotation {
                yaw: f64::MAX,
                pitch: f64::MAX,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Vector3};
    use std::f64::consts::PI;

    fn facing_forward() -> f64 {
        -PI / 2.0
    }

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "expected {a} to be close to {b}");
    }

    #[test]
    fn forward_moves_in_facing_direction() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        move_relative(&mut v, 0.0, 1.0, 0.1, facing_forward());
        assert_close(v.x, 0.0);
        assert_close(v.z, 0.1);
    }

    #[test]
    fn strafe_moves_perpendicular_to_facing() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        move_relative(&mut v, 1.0, 0.0, 0.1, facing_forward());
        assert_close(v.x, 0.1);
        assert_close(v.z, 0.0);
    }

    #[test]
    fn diagonal_is_normalized_to_full_speed() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        move_relative(&mut v, 1.0, 1.0, 0.1, facing_forward());
        let expected = 0.1 / 2.0f64.sqrt();
        assert_close(v.x, expected);
        assert_close(v.z, expected);
        assert_close(v.magnitude(), 0.1);
    }

    #[test]
    fn backward_moves_opposite_to_facing() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        move_relative(&mut v, 0.0, -1.0, 0.1, facing_forward());
        assert_close(v.x, 0.0);
        assert_close(v.z, -0.1);
    }

    #[test]
    fn no_input_does_not_move() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        move_relative(&mut v, 0.0, 0.0, 0.1, facing_forward());
        assert_close(v.x, 0.0);
        assert_close(v.z, 0.0);
    }

    #[test]
    fn sneak_scales_input_by_0_3() {
        let mut movement = PlayerMovement::new();
        movement.pressed_keys.insert(Actionkey::Forward, true);
        movement.pressed_keys.insert(Actionkey::Left, true);
        movement.pressed_keys.insert(Actionkey::Sneak, true);
        let (strafe, forward) = movement.movement_input();
        assert_close(strafe, 0.3);
        assert_close(forward, 0.3);
    }

    #[test]
    fn air_strafe_accelerates_slower_than_ground() {
        let yaw = facing_forward();
        let mut air = Vector3::new(0.0, 0.0, 0.0);
        let mut ground = Vector3::new(0.0, 0.0, 0.0);
        for _ in 0..5 {
            move_relative(&mut air, 1.0, 0.0, 0.02, yaw);
            air *= 0.91;
            move_relative(&mut ground, 1.0, 0.0, 0.1, yaw);
            ground *= 0.546;
        }
        assert!(ground.x > air.x * 1.4);
        assert!(air.x < 0.1);
        assert!(ground.x > 0.1);
    }

    #[test]
    fn sprint_jump_boost_matches_forward_direction() {
        // yaw = 0 faces +Z, so the boost must push along +Z
        let (x, z) = sprint_jump_boost(0.0);
        assert_close(x, 0.0);
        assert_close(z, 0.2);
        // yaw = PI/2 faces +X, so the boost must push along +X
        let (x, z) = sprint_jump_boost(PI / 2.0);
        assert_close(x, 0.2);
        assert_close(z, 0.0);
    }
}
