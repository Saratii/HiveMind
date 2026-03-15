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

// Stores the camera's current angular velocity in yaw and pitch axes, used to apply momentum-based coasting after the mouse button is released
// yaw_vel: current yaw angular velocity in radians per second, decays toward zero when not dragging
// pitch_vel: current pitch angular velocity in radians per second, decays toward zero when not dragging
#[derive(Resource, Default)]
pub struct OrbitMomentum {
    pub yaw_vel: f32,
    pub pitch_vel: f32,
}

// Updates the camera's yaw and pitch based on left-click mouse drag input with momentum, accelerating toward the current mouse velocity while dragging and coasting to a stop with exponential drag after release, then repositions the camera to orbit around the current focus point
// Input: state: ResMut<Orbit> holding the current orbital angles and focus; momentum: ResMut<OrbitMomentum> holding the current angular velocities; btn: Res<ButtonInput<MouseButton>> for detecting left-click hold; motion: Res<AccumulatedMouseMotion> providing the frame's mouse delta; time: Res<Time> for the frame delta; q: Query<&mut Transform, With<Camera3d>> to update the camera's world transform
// Returns: none
pub fn orbit_camera(
    mut state: ResMut<Orbit>,
    mut momentum: ResMut<OrbitMomentum>,
    btn: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    mut q: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    const DRAG: f32 = 4.0;
    const TRACKING: f32 = 30.0;
    if btn.pressed(MouseButton::Left) && motion.delta != Vec2::ZERO {
        let target_yaw_vel = -motion.delta.x * 0.005 / dt.max(0.001);
        let target_pitch_vel = motion.delta.y * 0.005 / dt.max(0.001);
        momentum.yaw_vel += (target_yaw_vel - momentum.yaw_vel) * (TRACKING * dt).min(1.0);
        momentum.pitch_vel += (target_pitch_vel - momentum.pitch_vel) * (TRACKING * dt).min(1.0);
    } else {
        let drag_factor = (-DRAG * dt).exp();
        momentum.yaw_vel *= drag_factor;
        momentum.pitch_vel *= drag_factor;
    }
    if momentum.yaw_vel.abs() < 0.001 && momentum.pitch_vel.abs() < 0.001 {
        momentum.yaw_vel = 0.0;
        momentum.pitch_vel = 0.0;
        return;
    }
    state.yaw += momentum.yaw_vel * dt;
    state.pitch = (state.pitch + momentum.pitch_vel * dt).clamp(0.05, PI / 2.0 - 0.05);
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
