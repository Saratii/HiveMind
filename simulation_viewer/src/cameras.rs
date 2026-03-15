/*
prologue
Name of program: cameras.rs
Description: Setup main camera rendering, lighting, and other visual effects.
Author: Maren Proplesch
Date Created: 3/13/2026
Date Revised: 3/13/2026
Revision History: None
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use std::f32::consts::PI;

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
};

use crate::{Orbit, orbit_pos};

// Updates the camera's yaw and pitch based on left-click mouse drag input, then repositions the camera to orbit around the current focus point
// Input: state: ResMut<Orbit> holding the current orbital angles and focus; btn: Res<ButtonInput<MouseButton>> for detecting left-click hold; motion: Res<AccumulatedMouseMotion> providing the frame's mouse delta; q: Query<&mut Transform, With<Camera3d>> to update the camera's world transform
// Returns: none
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

// Adjusts the camera's orbital radius based on scroll wheel input, clamping the distance between a minimum and maximum range, then repositions the camera accordingly
// Input: state: ResMut<Orbit> holding the current radius and focus; scroll: Res<AccumulatedMouseScroll> providing the frame's scroll delta; q: Query<&mut Transform, With<Camera3d>> to update the camera's world transform
// Returns: none
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
