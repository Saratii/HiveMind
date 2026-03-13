use std::f32::consts::PI;

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
};

use crate::{Orbit, orbit_pos};

pub fn orbit_camera(
    mut state: ResMut<Orbit>,
    btn: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut q: Query<&mut Transform, With<Camera3d>>,
) {
    if !btn.pressed(MouseButton::Left) || motion.delta == Vec2::ZERO {
        return;
    }
    state.yaw -= motion.delta.x * 0.005;
    state.pitch = (state.pitch - motion.delta.y * 0.005).clamp(0.05, PI / 2.0 - 0.05);
    if let Ok(mut t) = q.single_mut() {
        *t = Transform::from_translation(orbit_pos(&state)).looking_at(state.focus, Vec3::Y);
    }
}

pub fn zoom_camera(
    mut state: ResMut<Orbit>,
    scroll: Res<AccumulatedMouseScroll>,
    mut q: Query<&mut Transform, With<Camera3d>>,
) {
    if scroll.delta.y == 0.0 {
        return;
    }
    state.radius = (state.radius - scroll.delta.y * 100.0).clamp(150.0, 9000.0);
    if let Ok(mut t) = q.single_mut() {
        *t = Transform::from_translation(orbit_pos(&state)).looking_at(state.focus, Vec3::Y);
    }
}
