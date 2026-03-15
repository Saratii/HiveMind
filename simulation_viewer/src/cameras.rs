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
Citation: Used AI copilot for limited code generation - claude.ai
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

// Enumerates the two available camera control modes that the user can toggle between at runtime
#[derive(Resource, Default, PartialEq, Clone, Copy)]
pub enum CameraMode {
    #[default]
    Orbit,
    Fly,
}

// Stores the fly camera's current position and look angles so they persist across mode toggles
// yaw: horizontal look angle in radians, wraps freely
// pitch: vertical look angle in radians, clamped to prevent flipping
// position: world position of the camera, preserved when switching back to fly mode
#[derive(Resource)]
pub struct FlyCamState {
    pub yaw: f32,
    pub pitch: f32,
    pub position: Vec3,
}

impl Default for FlyCamState {
    // Returns a FlyCamState positioned at the default orbit camera position looking slightly downward
    // Input: none
    // Returns: FlyCamState with yaw and pitch matching the default orbit angle and position at the default orbit camera world location
    fn default() -> Self {
        use crate::Orbit;
        use crate::orbit_pos;
        let orbit = Orbit::default();
        let pos = orbit_pos(&orbit);
        Self {
            yaw: -std::f32::consts::PI / 4.0,
            pitch: -std::f32::consts::PI / 6.0,
            position: pos,
        }
    }
}

// Updates the camera's yaw and pitch based on left-click mouse drag input with momentum, accelerating toward the current mouse velocity while dragging and coasting to a stop with exponential drag after release, then repositions the camera to orbit around the world origin; no-ops when the camera is in Fly mode
// Input: mode: Res<CameraMode> gating this system to Orbit mode only; state: ResMut<Orbit> holding the current orbital angles and focus; momentum: ResMut<OrbitMomentum> holding the current angular velocities; btn: Res<ButtonInput<MouseButton>> for detecting left-click hold; motion: Res<AccumulatedMouseMotion> providing the frame's mouse delta; time: Res<Time> for the frame delta; q: Query<&mut Transform, With<Camera3d>> to update the camera's world transform
// Returns: none
pub fn orbit_camera(
    mode: Res<CameraMode>,
    mut state: ResMut<Orbit>,
    mut momentum: ResMut<OrbitMomentum>,
    btn: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    mut q: Query<&mut Transform, With<Camera3d>>,
) {
    if *mode != CameraMode::Orbit {
        return;
    }
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
        *t = Transform::from_translation(orbit_pos(&state)).looking_at(Vec3::ZERO, Vec3::Y);
    }
}

// Adjusts the camera's orbital radius based on scroll wheel input, clamping the distance between a minimum and maximum range, then repositions the camera accordingly; no-ops when the camera is in Fly mode
// Input: mode: Res<CameraMode> gating this system to Orbit mode only; state: ResMut<Orbit> holding the current radius and focus; scroll: Res<AccumulatedMouseScroll> providing the frame's scroll delta; q: Query<&mut Transform, With<Camera3d>> to update the camera's world transform
// Returns: none
pub fn zoom_camera(
    mode: Res<CameraMode>,
    mut state: ResMut<Orbit>,
    scroll: Res<AccumulatedMouseScroll>,
    mut q: Query<&mut Transform, With<Camera3d>>,
) {
    if *mode != CameraMode::Orbit {
        return;
    }
    if scroll.delta.y == 0.0 {
        return;
    }
    state.radius = (state.radius - scroll.delta.y * 100.0).clamp(150.0, 9000.0);
    if let Ok(mut t) = q.single_mut() {
        *t = Transform::from_translation(orbit_pos(&state)).looking_at(Vec3::ZERO, Vec3::Y);
    }
}

// Moves and rotates the camera freely in world space using WASD for horizontal movement, Space/Ctrl for vertical, and right-click drag for mouse-look; holding either Shift key multiplies movement speed by 4; persists position and rotation into FlyCamState each frame so they survive mode toggles; no-ops when the camera is in Orbit mode
// Input: mode: Res<CameraMode> gating this system to Fly mode only; fly_state: ResMut<FlyCamState> holding the persistent position and look angles; btn: Res<ButtonInput<MouseButton>> for detecting right-click hold; keys: Res<ButtonInput<KeyCode>> for WASD, vertical movement, and sprint modifier; motion: Res<AccumulatedMouseMotion> providing the frame's mouse delta; time: Res<Time> for the frame delta; q: Query<&mut Transform, With<Camera3d>> to update the camera's world transform
// Returns: none
pub fn flycam_system(
    mode: Res<CameraMode>,
    mut fly_state: ResMut<FlyCamState>,
    btn: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    mut q: Query<&mut Transform, With<Camera3d>>,
) {
    if *mode != CameraMode::Fly {
        return;
    }
    let dt = time.delta_secs();
    const BASE_SPEED: f32 = 400.0;
    const SPRINT_MULT: f32 = 4.0;
    const LOOK_SENS: f32 = 0.003;
    if btn.pressed(MouseButton::Right) && motion.delta != Vec2::ZERO {
        fly_state.yaw -= motion.delta.x * LOOK_SENS;
        fly_state.pitch =
            (fly_state.pitch - motion.delta.y * LOOK_SENS).clamp(-PI / 2.0 + 0.01, PI / 2.0 - 0.01);
    }
    let rotation = Quat::from_euler(EulerRot::YXZ, fly_state.yaw, fly_state.pitch, 0.0);
    let forward = rotation * Vec3::NEG_Z;
    let right = rotation * Vec3::X;
    let sprinting = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let speed = BASE_SPEED * if sprinting { SPRINT_MULT } else { 1.0 };
    let mut move_dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        move_dir += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        move_dir -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        move_dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        move_dir -= right;
    }
    if keys.pressed(KeyCode::Space) {
        move_dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyE) {
        move_dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        move_dir -= Vec3::Y;
    }
    if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        move_dir -= Vec3::Y;
    }
    if let Ok(mut t) = q.single_mut() {
        if move_dir.length_squared() > 0.0 {
            t.translation += move_dir.normalize() * speed * dt;
        }
        t.rotation = rotation;
        fly_state.position = t.translation;
    }
}
